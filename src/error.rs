use reqwest::StatusCode;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("invalid API payload: {0}")]
    InvalidPayload(String),
    #[error("request transport failed")]
    Transport(#[source] reqwest::Error),
    #[error("Predict request failed with HTTP {status}")]
    HttpStatus { status: StatusCode },
    #[error("JSON decode failed")]
    Json(#[source] serde_json::Error),
    #[error("URL construction failed")]
    Url(#[source] url::ParseError),
    #[error("order signing failed: {0}")]
    Signing(String),
}

impl Error {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::HttpStatus { status } => Some(*status),
            _ => None,
        }
    }

    pub fn is_not_found(&self) -> bool {
        self.status() == Some(StatusCode::NOT_FOUND)
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(
            self.status(),
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        )
    }

    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(StatusCode::TOO_MANY_REQUESTS)
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::HttpStatus { status } => {
                *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            _ => false,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::Transport(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<url::ParseError> for Error {
    fn from(value: url::ParseError) -> Self {
        Self::Url(value)
    }
}
