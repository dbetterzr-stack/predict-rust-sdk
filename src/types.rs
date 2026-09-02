use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradingStatus {
    Open,
    MatchingNotEnabled,
    CancelOnly,
    Closed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderStatus {
    Open,
    Filled,
    Cancelled,
    Expired,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub name: String,
    pub index_set: u64,
    pub on_chain_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub question: String,
    pub trading_status: TradingStatus,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_visible: bool,
    pub is_neg_risk: bool,
    pub is_yield_bearing: bool,
    pub fee_rate_bps: u64,
    pub outcomes: Vec<Outcome>,
    #[serde(default)]
    pub category_slug: String,
    #[serde(default = "default_decimal_precision")]
    pub decimal_precision: u32,
    #[serde(default)]
    pub market_variant: String,
    #[serde(default)]
    pub variant_data: serde_json::Value,
}

fn default_decimal_precision() -> u32 {
    2
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryOutcomes {
    pub positive: Outcome,
    pub negative: Outcome,
}

impl Market {
    /// Strictly binds two outcome labels without ever falling back to array order.
    pub fn bind_binary_outcomes(
        &self,
        positive_labels: &[&str],
        negative_labels: &[&str],
    ) -> Result<BinaryOutcomes> {
        if self.outcomes.len() != 2 {
            return Err(Error::InvalidPayload(format!(
                "market {} must contain exactly two outcomes",
                self.id
            )));
        }
        let positives = self
            .outcomes
            .iter()
            .filter(|outcome| label_matches(&outcome.name, positive_labels))
            .collect::<Vec<_>>();
        let negatives = self
            .outcomes
            .iter()
            .filter(|outcome| label_matches(&outcome.name, negative_labels))
            .collect::<Vec<_>>();
        if positives.len() != 1 || negatives.len() != 1 {
            return Err(Error::InvalidPayload(format!(
                "market {} has no unique binary label mapping",
                self.id
            )));
        }
        let positive = positives[0];
        let negative = negatives[0];
        if positive.on_chain_id.trim().is_empty()
            || negative.on_chain_id.trim().is_empty()
            || positive.on_chain_id == negative.on_chain_id
            || positive.index_set == negative.index_set
        {
            return Err(Error::InvalidPayload(format!(
                "market {} has invalid or duplicate outcome identity",
                self.id
            )));
        }
        Ok(BinaryOutcomes {
            positive: positive.clone(),
            negative: negative.clone(),
        })
    }
}

fn label_matches(value: &str, expected: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    expected
        .iter()
        .any(|candidate| normalized == candidate.trim().to_ascii_lowercase())
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i64,
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    pub is_neg_risk: bool,
    pub is_yield_bearing: bool,
    #[serde(default)]
    pub starts_at: Option<String>,
    #[serde(default)]
    pub ends_at: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_visible: bool,
    #[serde(default)]
    pub markets: Vec<Market>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedOrder {
    pub hash: String,
    pub salt: String,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    pub token_id: String,
    pub maker_amount: String,
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    pub fee_rate_bps: String,
    pub side: u8,
    pub signature_type: u8,
    pub signature: String,
}

impl std::fmt::Debug for SignedOrder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignedOrder")
            .field("hash", &self.hash)
            .field("token_id", &self.token_id)
            .field("side", &self.side)
            .field("signature", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderData {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub order_hash: Option<String>,
    #[serde(default)]
    pub removal_locked_until: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    pub order: SignedOrder,
    pub id: String,
    pub market_id: i64,
    #[serde(default)]
    pub amount: String,
    #[serde(default)]
    pub amount_filled: String,
    pub is_neg_risk: bool,
    pub is_yield_bearing: bool,
    #[serde(default)]
    pub strategy: String,
    pub status: OrderStatus,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrdersData {
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub noop: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market(outcomes: Vec<Outcome>) -> Market {
        Market {
            id: 42,
            title: String::new(),
            question: "BTC Up or Down?".to_string(),
            trading_status: TradingStatus::Open,
            status: "REGISTERED".to_string(),
            is_visible: true,
            is_neg_risk: false,
            is_yield_bearing: false,
            fee_rate_bps: 0,
            outcomes,
            category_slug: "btc-updown-5m-1800000000".to_string(),
            decimal_precision: 2,
            market_variant: "CRYPTO_UP_DOWN".to_string(),
            variant_data: serde_json::json!({"type": "CRYPTO_UP_DOWN"}),
        }
    }

    #[test]
    fn binds_reversed_up_down_by_label() {
        let value = market(vec![
            Outcome {
                name: "Down".to_string(),
                index_set: 2,
                on_chain_id: "22".to_string(),
            },
            Outcome {
                name: "Up".to_string(),
                index_set: 1,
                on_chain_id: "11".to_string(),
            },
        ]);
        let binding = value.bind_binary_outcomes(&["Up"], &["Down"]).unwrap();
        assert_eq!(binding.positive.on_chain_id, "11");
        assert_eq!(binding.negative.on_chain_id, "22");
    }

    #[test]
    fn rejects_positional_fallback_and_duplicate_tokens() {
        let unknown = market(vec![
            Outcome {
                name: "A".to_string(),
                index_set: 1,
                on_chain_id: "11".to_string(),
            },
            Outcome {
                name: "B".to_string(),
                index_set: 2,
                on_chain_id: "22".to_string(),
            },
        ]);
        assert!(unknown.bind_binary_outcomes(&["Up"], &["Down"]).is_err());

        let duplicate = market(vec![
            Outcome {
                name: "Up".to_string(),
                index_set: 1,
                on_chain_id: "11".to_string(),
            },
            Outcome {
                name: "Down".to_string(),
                index_set: 2,
                on_chain_id: "11".to_string(),
            },
        ]);
        assert!(duplicate.bind_binary_outcomes(&["Up"], &["Down"]).is_err());
    }
}
