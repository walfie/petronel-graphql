use crate::error::Result;
use crate::model::RaidWithImage;
use crate::twitter::model::Tweet;
use futures_util::stream::{Stream, StreamExt};
use std::convert::TryFrom;
use twitter_stream::Token;

const TRACK: &'static str = "参加者募集！,:参戦ID,I need backup!,:Battle ID";

fn handle_msg<T>(msg: T) -> Result<Option<RaidWithImage>>
where
    T: AsRef<[u8]>,
{
    let tweet = serde_json::from_slice::<Tweet>(msg.as_ref())?;
    Ok(RaidWithImage::try_from(tweet).ok())
}

pub async fn connect(token: Token) -> Result<impl Stream<Item = Result<RaidWithImage>>> {
    let stream = twitter_stream::Builder::new(token)
        .track(TRACK)
        .listen()
        .await?
        .filter_map(|result| async {
            match result {
                Ok(msg) => handle_msg(msg.into_inner()).transpose(),
                Err(e) => Some(Err(e.into())),
            }
        });

    Ok(stream)
}
