use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("http {0}")]
    Http(u16),
    #[error("not found")]
    NotFound,
    #[error("rate limited")]
    RateLimited,
    #[error("cancelled")]
    Cancelled,
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, ApiError>;

impl From<tuikit::http::HttpError> for ApiError {
    fn from(value: tuikit::http::HttpError) -> Self {
        match value {
            tuikit::http::HttpError::Network(msg) => ApiError::Network(msg),
            tuikit::http::HttpError::Status { status } => ApiError::Http(status),
            tuikit::http::HttpError::RateLimited => ApiError::RateLimited,
            tuikit::http::HttpError::NotFound => ApiError::NotFound,
            tuikit::http::HttpError::TooLarge => ApiError::Network("response is too large".into()),
            tuikit::http::HttpError::InvalidBody(msg) => ApiError::Network(format!("invalid response: {msg}")),
        }
    }
}

