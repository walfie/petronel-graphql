use futures_util::stream::StreamExt;
use petronel_graphql::twitter::{connect, Token};
use std::env::VarError;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let conn = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(conn);

    let mut stream = connect(client, token_from_env()?).await?;
    while let Some(item) = stream.next().await {
        let _ = dbg!(item);
    }

    Ok(())
}

fn env(name: &str) -> Result<String, VarError> {
    Ok(std::env::var(name)?)
}

fn token_from_env() -> anyhow::Result<Token, VarError> {
    Ok(Token::new(
        env("CONSUMER_KEY")?,
        env("CONSUMER_SECRET")?,
        env("ACCESS_TOKEN")?,
        env("ACCESS_TOKEN_SECRET")?,
    ))
}
