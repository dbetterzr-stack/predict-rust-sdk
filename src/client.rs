use crate::{
    AccountSigner, CancelOrdersData, Category, CreateOrderData, Error, Market, OpenOrder,
    OrderStatus, Page, PreparedOrder, Result, MAINNET_BASE_URL,
};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client, RequestBuilder, Response,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{fmt, sync::Arc, time::Duration};
use url::Url;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateLimitSnapshot {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset: Option<String>,
}

impl RateLimitSnapshot {
    fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            limit: first_u64_header(
                headers,
                &["ratelimit-limit", "x-ratelimit-limit", "x-rate-limit-limit"],
            ),
            remaining: first_u64_header(
                headers,
                &[
                    "ratelimit-remaining",
                    "x-ratelimit-remaining",
                    "x-rate-limit-remaining",
                ],
            ),
            reset: first_string_header(
                headers,
                &["ratelimit-reset", "x-ratelimit-reset", "x-rate-limit-reset"],
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiResponse<T> {
    pub data: T,
    pub rate_limit: RateLimitSnapshot,
}

#[derive(Clone)]
pub struct AccessToken(Arc<Zeroizing<String>>);

impl AccessToken {
    fn new(token: String) -> Result<Self> {
        if token.trim().is_empty() {
            return Err(Error::InvalidPayload(
                "authentication response contained an empty token".to_string(),
            ));
        }
        Ok(Self(Arc::new(Zeroizing::new(token))))
    }

    fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccessToken([REDACTED])")
    }
}

#[derive(Clone)]
pub struct PredictClient {
    http: Client,
    base_url: Url,
    api_key: Arc<Zeroizing<String>>,
}

impl fmt::Debug for PredictClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PredictClient")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl PredictClient {
    pub fn mainnet(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(MAINNET_BASE_URL, api_key)
    }

    pub fn with_base_url(base_url: &str, api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(Error::Config("API key cannot be empty".to_string()));
        }
        let mut normalized = base_url.trim_end_matches('/').to_string();
        normalized.push('/');
        let base_url = Url::parse(&normalized)?;
        if base_url.scheme() != "https" && !is_loopback_url(&base_url) {
            return Err(Error::Config(
                "non-loopback API endpoints must use HTTPS".to_string(),
            ));
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base_url,
            api_key: Arc::new(Zeroizing::new(api_key)),
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub async fn get_category(&self, slug: &str) -> Result<ApiResponse<Category>> {
        let slug = require_path_value(slug, "category slug")?;
        let response = self
            .api_key_request(
                self.http
                    .get(self.endpoint(&format!("categories/{}", urlencoding::encode(slug)))?),
            )?
            .send()
            .await?;
        self.decode_data(response).await
    }

    pub async fn get_market(&self, market_id: i64) -> Result<ApiResponse<Market>> {
        if market_id <= 0 {
            return Err(Error::Config("market_id must be positive".to_string()));
        }
        let response = self
            .api_key_request(
                self.http
                    .get(self.endpoint(&format!("markets/{market_id}"))?),
            )?
            .send()
            .await?;
        self.decode_data(response).await
    }

    pub async fn authenticate(&self, signer: &AccountSigner) -> Result<ApiResponse<AccessToken>> {
        let message_response = self
            .api_key_request(self.http.get(self.endpoint("auth/message")?))?
            .send()
            .await?;
        let ApiResponse {
            data: auth_message,
            rate_limit: first_rate_limit,
        } = self.decode_data::<AuthMessage>(message_response).await?;
        let auth_payload = signer.auth_payload(&auth_message.message)?;
        let token_response = self
            .api_key_request(self.http.post(self.endpoint("auth")?))?
            .json(&auth_payload)
            .send()
            .await?;
        let ApiResponse {
            data: auth_token,
            rate_limit,
        } = self.decode_data::<AuthToken>(token_response).await?;
        Ok(ApiResponse {
            data: AccessToken::new(auth_token.token)?,
            rate_limit: merge_rate_limits(first_rate_limit, rate_limit),
        })
    }

    pub async fn submit_order(
        &self,
        jwt: &AccessToken,
        order: &PreparedOrder,
    ) -> Result<ApiResponse<CreateOrderData>> {
        let response = self
            .authenticated_request(self.http.post(self.endpoint("orders")?), jwt)?
            .json(order.payload())
            .send()
            .await?;
        self.decode_data(response).await
    }

    pub async fn get_order_by_hash(
        &self,
        jwt: &AccessToken,
        order_hash: &str,
    ) -> Result<ApiResponse<OpenOrder>> {
        let order_hash = require_path_value(order_hash, "order hash")?;
        let response = self
            .authenticated_request(
                self.http
                    .get(self.endpoint(&format!("orders/{}", urlencoding::encode(order_hash)))?),
                jwt,
            )?
            .send()
            .await?;
        self.decode_data(response).await
    }

    pub async fn list_orders(
        &self,
        jwt: &AccessToken,
        status: OrderStatus,
        first: u16,
        after: Option<&str>,
    ) -> Result<ApiResponse<Page<OpenOrder>>> {
        if first == 0 || first > 100 {
            return Err(Error::Config("first must be between 1 and 100".to_string()));
        }
        let status = order_status_query(status)?;
        let mut request = self
            .http
            .get(self.endpoint("orders")?)
            .query(&[("first", first.to_string()), ("status", status.to_string())]);
        if let Some(cursor) = after.filter(|value| !value.trim().is_empty()) {
            request = request.query(&[("after", cursor)]);
        }
        let response = self.authenticated_request(request, jwt)?.send().await?;
        let decoded = self.decode_envelope::<Vec<OpenOrder>>(response).await?;
        Ok(ApiResponse {
            data: Page {
                data: decoded.data,
                cursor: decoded.cursor,
            },
            rate_limit: decoded.rate_limit,
        })
    }

    pub async fn remove_orders(
        &self,
        jwt: &AccessToken,
        ids: &[String],
    ) -> Result<ApiResponse<CancelOrdersData>> {
        if ids.is_empty() || ids.len() > 10 || ids.iter().any(|id| id.trim().is_empty()) {
            return Err(Error::Config(
                "remove_orders requires 1 to 10 non-empty IDs".to_string(),
            ));
        }
        let response = self
            .authenticated_request(self.http.post(self.endpoint("orders/remove")?), jwt)?
            .json(&json!({"data": {"ids": ids}}))
            .send()
            .await?;
        let rate_limit = RateLimitSnapshot::from_headers(response.headers());
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::HttpStatus { status });
        }
        #[derive(Deserialize)]
        struct RemoveEnvelope {
            success: bool,
            #[serde(default)]
            removed: Vec<String>,
            #[serde(default)]
            noop: Vec<String>,
        }
        let payload: RemoveEnvelope = serde_json::from_slice(&bytes)?;
        if !payload.success {
            return Err(Error::InvalidPayload(
                "remove_orders returned success=false".to_string(),
            ));
        }
        Ok(ApiResponse {
            data: CancelOrdersData {
                removed: payload.removed,
                noop: payload.noop,
            },
            rate_limit,
        })
    }

    fn endpoint(&self, relative: &str) -> Result<Url> {
        Ok(self.base_url.join(relative)?)
    }

    fn api_key_request(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let mut value = HeaderValue::from_str(self.api_key.as_str())
            .map_err(|_| Error::Config("API key contains invalid header bytes".to_string()))?;
        value.set_sensitive(true);
        Ok(request.header("x-api-key", value))
    }

    fn authenticated_request(
        &self,
        request: RequestBuilder,
        jwt: &AccessToken,
    ) -> Result<RequestBuilder> {
        let request = self.api_key_request(request)?;
        let mut value = HeaderValue::from_str(&format!("Bearer {}", jwt.expose()))
            .map_err(|_| Error::Config("JWT contains invalid header bytes".to_string()))?;
        value.set_sensitive(true);
        Ok(request.header(AUTHORIZATION, value))
    }

    async fn decode_data<T: DeserializeOwned>(&self, response: Response) -> Result<ApiResponse<T>> {
        let envelope = self.decode_envelope::<T>(response).await?;
        Ok(ApiResponse {
            data: envelope.data,
            rate_limit: envelope.rate_limit,
        })
    }

    async fn decode_envelope<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> Result<DecodedEnvelope<T>> {
        let rate_limit = RateLimitSnapshot::from_headers(response.headers());
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::HttpStatus { status });
        }
        #[derive(Deserialize)]
        struct Envelope {
            success: bool,
            #[serde(default)]
            data: Option<Value>,
            #[serde(default)]
            cursor: Option<String>,
        }
        let envelope: Envelope = serde_json::from_slice(&bytes)?;
        if !envelope.success {
            return Err(Error::InvalidPayload(
                "Predict response returned success=false".to_string(),
            ));
        }
        let data = envelope
            .data
            .ok_or_else(|| Error::InvalidPayload("Predict response omitted data".to_string()))?;
        Ok(DecodedEnvelope {
            data: serde_json::from_value(data)?,
            cursor: envelope.cursor,
            rate_limit,
        })
    }
}

#[derive(Debug)]
struct DecodedEnvelope<T> {
    data: T,
    cursor: Option<String>,
    rate_limit: RateLimitSnapshot,
}

#[derive(Debug, Deserialize)]
struct AuthMessage {
    message: String,
}

#[derive(Debug, Deserialize)]
struct AuthToken {
    token: String,
}

fn first_u64_header(headers: &HeaderMap, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
    })
}

fn first_string_header(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    })
}

fn merge_rate_limits(first: RateLimitSnapshot, second: RateLimitSnapshot) -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit: second.limit.or(first.limit),
        remaining: second.remaining.or(first.remaining),
        reset: second.reset.or(first.reset),
    }
}

fn require_path_value<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(Error::Config(format!("{label} cannot be empty")))
    } else {
        Ok(value)
    }
}

fn order_status_query(status: OrderStatus) -> Result<&'static str> {
    match status {
        OrderStatus::Open => Ok("OPEN"),
        OrderStatus::Filled => Ok("FILLED"),
        OrderStatus::Cancelled => Ok("CANCELLED"),
        OrderStatus::Expired => Ok("EXPIRED"),
        OrderStatus::Failed => Ok("FAILED"),
        OrderStatus::Unknown => Err(Error::Config(
            "cannot query an unknown order status".to_string(),
        )),
    }
}

fn is_loopback_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn category_json() -> Value {
        json!({
            "success": true,
            "data": {
                "id": 7,
                "slug": "btc-updown-5m-1800000000",
                "title": "BTC Up or Down",
                "createdAt": "2027-01-14T08:00:02.000Z",
                "publishedAt": "2027-01-14T08:00:36.807Z",
                "isNegRisk": false,
                "isYieldBearing": false,
                "startsAt": "2027-01-15T08:00:00Z",
                "endsAt": "2027-01-15T08:05:00Z",
                "status": "OPEN",
                "isVisible": true,
                "markets": [{
                    "id": 9,
                    "question": "BTC Up or Down?",
                    "tradingStatus": "OPEN",
                    "status": "REGISTERED",
                    "isVisible": true,
                    "isNegRisk": false,
                    "isYieldBearing": false,
                    "feeRateBps": 0,
                    "outcomes": [
                        {"name": "Down", "indexSet": 2, "onChainId": "22"},
                        {"name": "Up", "indexSet": 1, "onChainId": "11"}
                    ],
                    "categorySlug": "btc-updown-5m-1800000000",
                    "decimalPrecision": 2,
                    "marketVariant": "CRYPTO_UP_DOWN",
                    "variantData": {"type": "CRYPTO_UP_DOWN"}
                }]
            }
        })
    }

    #[tokio::test]
    async fn parses_category_and_rate_limit_without_exposing_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories/btc-updown-5m-1800000000"))
            .and(header("x-api-key", "synthetic-api-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-ratelimit-limit", "240")
                    .insert_header("x-ratelimit-remaining", "239")
                    .set_body_json(category_json()),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client =
            PredictClient::with_base_url(&format!("{}/v1", server.uri()), "synthetic-api-key")
                .unwrap();
        let response = client
            .get_category("btc-updown-5m-1800000000")
            .await
            .unwrap();
        assert_eq!(response.data.markets[0].id, 9);
        assert_eq!(
            response.data.created_at.as_deref(),
            Some("2027-01-14T08:00:02.000Z")
        );
        assert_eq!(
            response.data.published_at.as_deref(),
            Some("2027-01-14T08:00:36.807Z")
        );
        assert_eq!(response.rate_limit.limit, Some(240));
        assert_eq!(response.rate_limit.remaining, Some(239));
        assert!(!format!("{client:?}").contains("synthetic-api-key"));
    }

    #[tokio::test]
    async fn classifies_not_found_without_echoing_response_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories/missing"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_string("sensitive server body must not enter the error"),
            )
            .mount(&server)
            .await;
        let client =
            PredictClient::with_base_url(&format!("{}/v1", server.uri()), "test-key").unwrap();
        let error = client.get_category("missing").await.unwrap_err();
        assert!(error.is_not_found());
        assert!(!error.to_string().contains("sensitive"));
    }

    #[test]
    fn access_token_debug_is_redacted() {
        let token = AccessToken::new("header.payload.signature".to_string()).unwrap();
        assert_eq!(format!("{token:?}"), "AccessToken([REDACTED])");
    }

    #[test]
    fn retry_classification_is_conservative() {
        assert!(Error::HttpStatus {
            status: StatusCode::TOO_MANY_REQUESTS
        }
        .is_retryable());
        assert!(Error::HttpStatus {
            status: StatusCode::SERVICE_UNAVAILABLE
        }
        .is_retryable());
        assert!(!Error::HttpStatus {
            status: StatusCode::BAD_REQUEST
        }
        .is_retryable());
    }
}
