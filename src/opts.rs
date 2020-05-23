use std::time::Duration;
use structopt::StructOpt;

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

#[derive(Debug, StructOpt)]
pub struct Options {
    /// Twitter consumer key
    #[structopt(long, env, hide_env_values = true)]
    pub consumer_key: String,

    /// Twitter consumer secret
    #[structopt(long, env, hide_env_values = true)]
    pub consumer_secret: String,

    /// Twitter access token
    #[structopt(long, env, hide_env_values = true)]
    pub access_token: String,

    /// Twitter access token secret
    #[structopt(long, env, hide_env_values = true)]
    pub access_token_secret: String,

    /// Emit logs as structured JSON
    #[structopt(long, env)]
    pub json_logs: bool,

    /// If disconnected from the Twitter streaming API, wait this long before reconnecting
    #[structopt(long, env, default_value = "10s", parse(try_from_str = parse_duration))]
    pub connection_retry_delay: Duration,

    /// Reconnects to the Twitter streaming API if no messages are received in this amount of time
    #[structopt(long, env, default_value = "60s", parse(try_from_str = parse_duration))]
    pub connection_timeout: Duration,

    /// Number of tweets to retain for each boss
    #[structopt(long, env, default_value = "10")]
    pub raid_history_size: usize,

    /// Number of tweets and boss updates to keep around if consumers are lagging
    #[structopt(long, env, default_value = "10")]
    pub broadcast_capacity: usize,

    /// Max number of in-flight requests for boss image hashes
    #[structopt(long, env, default_value = "5")]
    pub image_hash_concurrency: usize,

    /// How often to run cleanup tasks
    ///
    /// This includes removing outdated bosses, removing broadcast channels for unknown bosses with
    /// no subscribers, etc.
    #[structopt(long, env, default_value = "5m", parse(try_from_str = parse_duration))]
    pub cleanup_interval: Duration,

    /// Bosses not seen for this long will be removed during cleanup tasks
    ///
    /// E.g., `15d` means any boss not seen in 15 days will be removed
    #[structopt(long, env, default_value = "15d", parse(try_from_str = parse_duration))]
    pub boss_max_age: Duration,

    /// Bind IP for the HTTP server
    #[structopt(long, short, env, default_value = "127.0.0.1")]
    pub bind_ip: String,

    /// Bind port for the HTTP server
    #[structopt(long, short, env, default_value = "8080")]
    pub port: u16,
}
