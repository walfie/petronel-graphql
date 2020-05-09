use crate::error::{Error, Result};
use crate::model::RaidWithImage;
use crate::twitter::model::Tweet;
use futures_util::future::ready;
use futures_util::stream::{Stream, StreamExt};
use hyper::body::HttpBody;
use hyper::http::Response;
use std::convert::TryFrom;
use twitter_stream::service::HttpService;
use twitter_stream::Token;

const TRACK: &'static str = "参加者募集！,:参戦ID,I need backup!,:Battle ID";

fn handle_msg(msg: &str) -> Result<Option<RaidWithImage>> {
    let tweet = serde_json::from_str::<Tweet>(msg)?;
    Ok(RaidWithImage::try_from(tweet).ok())
}

pub async fn connect<S, B>(
    service: S,
    token: Token,
) -> Result<impl Stream<Item = Result<RaidWithImage>>, twitter_stream::Error<S::Error>>
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
