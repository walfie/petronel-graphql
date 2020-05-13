use std::time::Duration;

use crate::error::{Error, Result};
use crate::model::Raid;
use crate::twitter::model::Tweet;

use futures::future::ready;
use futures::stream::{Stream, StreamExt};
use hyper::body::HttpBody;
use hyper::http::{Response, StatusCode};
use std::convert::TryFrom;
use std::fmt;
use std::future::Future;
use tokio::sync::mpsc;
use twitter_stream::service::HttpService;
use twitter_stream::Token;

const TRACK: &'static str = "参加者募集！,:参戦ID,I need backup!,:Battle ID";

fn handle_msg(msg: &str) -> Result<Option<Raid>> {
    let tweet = serde_json::from_str::<Tweet>(msg)?;
    Ok(Raid::try_from(tweet).ok())
}

pub async fn connect<S, B>(
    service: S,
    token: Token,
) -> Result<impl Stream<Item = Result<Raid>>, twitter_stream::Error<S::Error>>
where
    S: HttpService<B, Response = Response<B>>,
    B: From<Vec<u8>> + HttpBody,
    Error: From<twitter_stream::Error<B::Error>>,
{
    let stream = twitter_stream::Builder::new(token)
        .track(TRACK)
        .listen_with_client(service)
        .await?
        .filter_map(|result| {
            ready({
                match result {
                    Ok(msg) => handle_msg(&msg).transpose(),
                    Err(e) => Some(Err(e.into())),
                }
            })
        });

    Ok(stream)
}

fn is_retryable(status: StatusCode) -> bool {
    // 4xx errors should not be retried unless it's due to rate limiting (status 420 or 429)
    if status.is_client_error() {
        status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 420
    } else {
        true
    }
}

pub fn connect_with_retries<S, B>(
    log: slog::Logger,
    service: S,
    token: Token,
    retry_delay: Duration,
    timeout: Duration,
) -> (impl Stream<Item = Raid>, impl Future<Output = Error>)
where
    S: HttpService<B, Response = Response<B>> + Clone,
    S::Error: fmt::Display,
    B: From<Vec<u8>> + HttpBody + Unpin,
    Error: From<twitter_stream::Error<B::Error>>,
{
    let (tx, rx) = mpsc::unbounded_channel();

    let worker = async move {
        // Loop per connection attempt
        loop {
            use twitter_stream::Error::Http;
            match connect(service.clone(), token.clone()).await {
                // Loop per message
                Ok(mut stream) => loop {
                    match tokio::time::timeout(timeout, stream.next()).await {
                        Err(_) => {
                            slog::warn!(log, "Twitter stream timed out"; "duration" => ?timeout);
                        }
                        Ok(Some(Ok(msg))) => {
                            if let Err(_) = tx.send(msg) {
                                // Stream closed by receiver
                                return Error::Closed;
                            }
                        }
                        Ok(Some(Err(e))) => {
                            slog::warn!(log, "Error reading message from Twitter stream"; "error" => %e);
                        }
                        Ok(None) => {
                            slog::warn!(log, "Twitter stream ended");
                            break;
                        }
                    }
                },

                Err(Http(status)) if is_retryable(status) => {
                    slog::warn!(log, "Twitter HTTP error"; "statusCode" => status.as_u16());
                }
                Err(Http(status)) => {
                    slog::error!(log, "Non-retryable Twitter HTTP error code"; "error" => %status);
                    return Error::Http(status);
                }
                Err(e) => {
                    slog::warn!(log, "Twitter stream connection error"; "error" => %e);
                }
            };

            tokio::time::delay_for(retry_delay).await;
        }
    };

    (rx, worker)
}
