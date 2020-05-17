use http::StatusCode;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Twitter error: {0}")]
    Twitter(#[from] twitter_stream::hyper::Error),
    #[error("HTTP error: {0}")]
    Http(StatusCode),
    #[error("HTTP client error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("failed to load image: {0}")]
    Image(#[from] image::error::ImageError),
    #[error("failed to parse URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),
    #[error("stream was closed by receiver")]
    StreamClosed,
    #[error("invalid bind address: {0}")]
    BindPort(#[from] std::net::AddrParseError),
}
