use hyper::http::StatusCode;
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
    #[error("stream was closed by receiver")]
    Closed,
}
