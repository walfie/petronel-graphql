mod log;

use futures_util::stream::StreamExt;
use petronel_graphql::twitter::{connect_with_retries, Token};
use structopt::StructOpt;
use tokio::time::Duration;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(long = "json-logs", env = "JSON_LOGS")]
    json_logs: bool,
    #[structopt(
        long = "connection-retry-delay",
        env = "CONNECTION_RETRY_DELAY",
        default_value = "10s",
        parse(try_from_str = parse_duration)
    )]
    connection_retry_delay: Duration,
    #[structopt(
        long = "connection-timeout",
        env = "CONNECTION_TIMEOUT",
        default_value = "60s",
        parse(try_from_str = parse_duration)
    )]
    connection_timeout: Duration,

    #[structopt(env = "CONSUMER_KEY", hide_env_values = true)]
    consumer_key: String,
    #[structopt(env = "CONSUMER_SECRET", hide_env_values = true)]
    consumer_secret: String,
    #[structopt(env = "ACCESS_TOKEN", hide_env_values = true)]
    access_token: String,
    #[structopt(env = "ACCESS_TOKEN_SECRET", hide_env_values = true)]
    access_token_secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::from_args();

    let token = Token::new(
        opt.consumer_key,
        opt.consumer_secret,
        opt.access_token,
        opt.access_token_secret,
    );

    let log = log::logger(opt.json_logs);

    let conn = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(conn);

    let (mut stream, worker) = connect_with_retries(
        log,
        client,
        token,
        opt.connection_retry_delay,
        opt.connection_timeout,
    );
    tokio::spawn(worker);

    while let Some(item) = stream.next().await {
        let _ = dbg!(item);
    }

    Ok(())
}

fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    fn trim<F>(s: &str, suffix: &str, f: F) -> Option<Duration>
    where
        F: Fn(u64) -> Duration,
    {
        if s.ends_with(suffix) {
            s.trim_end_matches(suffix).parse::<u64>().map(f).ok()
        } else {
            None
        }
    }

    trim(s, "ms", Duration::from_millis)
        .or_else(|| trim(s, "s", Duration::from_secs))
        .or_else(|| trim(s, "m", |m| Duration::from_secs(m * 60)))
        .or_else(|| trim(s, "h", |h| Duration::from_secs(h * 60 * 60)))
        .or_else(|| trim(s, "d", |d| Duration::from_secs(d * 60 * 60 * 24)))
        .ok_or_else(|| anyhow::Error::msg("failed to parse duration"))
}
