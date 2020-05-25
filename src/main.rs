mod log;
mod opts;

use std::net::SocketAddr;
use std::time::Duration;

use chrono::Utc;
use futures::stream::StreamExt;
use petronel_graphql::image_hash::HyperImageHasher;
use petronel_graphql::model::Boss;
use petronel_graphql::persistence::{JsonFile, Persistence, Redis};
use petronel_graphql::{image_hash, twitter, RaidHandler};
use structopt::StructOpt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = opts::Options::from_args();

    let bind_addr: SocketAddr = format!("{}:{}", opt.bind_ip, opt.port).parse()?;

    let log = log::logger(opt.json_logs);

    let conn = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(conn);

    // Get boss list from cache
    let json_file = opt.storage_file_path.map(JsonFile::new);
    let redis_client = match opt.storage_redis_uri {
        None => None,
        Some(uri) => match Redis::new(uri, opt.storage_redis_key).await {
            Ok(client) => Some(client),
            Err(e) => {
                slog::warn!(log, "Failed to connect to Redis"; "error" => %e);
                None
            }
        },
    };

    let initial_bosses =
        get_initial_bosses(&log, json_file.as_ref(), redis_client.as_ref()).await?;
    let bosses_to_request_hashes_for = initial_bosses
        .iter()
        .filter(|b| b.needs_image_hash_update())
        .cloned()
        .collect::<Vec<_>>();

    // Initialize boss handler
    let raid_handler = RaidHandler::new(
        initial_bosses,
        opt.raid_history_size,
        opt.broadcast_capacity,
    );

    // Fetch boss images and calculate image hashes
    let hash_updater = image_hash::Updater::new(
        log.clone(),
        HyperImageHasher::new(client.clone()),
        raid_handler.clone(),
        opt.image_hash_concurrency,
    );
    let (hash_inbox, hash_worker) = hash_updater.run();
    bosses_to_request_hashes_for
        .iter()
        .for_each(|boss| hash_inbox.request_hash_for_boss(boss));
    tokio::spawn(hash_worker);

    // Cleanup task that runs on startup and periodically:
    // * removes bosses that haven't been seen in a while
    // * drops broadcast channels for bosses that don't exist and have no subscribers
    // * requests image hashes for bosses that have an image but no hash
    //   (possibly due to a failed HTTP request)
    tokio::spawn({
        let ttl = chrono::Duration::from_std(opt.boss_ttl)?;
        let raid_handler = raid_handler.clone();
        let mut interval = tokio::time::interval(opt.cleanup_interval);

        async move {
            loop {
                interval.tick().await;
                let long_ago = Utc::now() - ttl;
                raid_handler.retain(|entry| {
                    let boss = entry.boss();
                    if boss.needs_image_hash_update() {
                        hash_inbox.request_hash_for_boss(boss);
                    }

                    boss.last_seen_at.as_datetime() > long_ago
                });
            }
        }
    });

    // Periodically write boss data to JSON file
    if let Some(file) = json_file {
        let log = log.clone();
        tokio::spawn(save_bosses(
            raid_handler.clone(),
            file,
            opt.storage_file_flush_interval,
            move |file, result| match result {
                Ok(count) => {
                    slog::debug!(log, "Saved boss data to file"; "path" => file.path(), "count" => count)
                }
                Err(e) => {
                    slog::warn!(log, "Failed to save boss data to file"; "error" => %e, "path" => file.path())
                }
            },
        ));
    }

    // Periodically write boss data to Redis
    if let Some(redis) = redis_client {
        let log = log.clone();
        tokio::spawn(save_bosses(
            raid_handler.clone(),
            redis,
            opt.storage_redis_flush_interval,
            move |_, result| match result {
                Ok(count) => slog::debug!(log, "Saved boss data to Redis"; "count" => count),
                Err(e) => slog::warn!(log, "Failed to save boss data to Redis"; "error" => %e),
            },
        ));
    };

    // Start Twitter stream
    let token = twitter::Token::new(
        opt.consumer_key,
        opt.consumer_secret,
        opt.access_token,
        opt.access_token_secret,
    );
    let (mut tweet_stream, twitter_worker) = twitter::connect_with_retries(
        log.clone(),
        client,
        token,
        opt.connection_retry_delay,
        opt.connection_timeout,
    );

    let routes = petronel_graphql::graphql::routes(raid_handler.clone());
    tokio::spawn(async move {
        while let Some(item) = tweet_stream.next().await {
            raid_handler.push(item);
        }
    });

    // Start HTTP listeners
    slog::info!(log, "Starting HTTP server"; "port" => opt.port, "ip" => &opt.bind_ip);
    let server = tokio::spawn(warp::serve(routes).try_bind(bind_addr));

    tokio::select! {
        _ = twitter_worker => {
            slog::error!(log, "Disconnected from Twitter stream");
        }
        _ = server => {
            slog::error!(
                log, "Could not bind to the requested address";
                "port" => opt.port, "ip" => &opt.bind_ip
            );
        }
    };

    anyhow::bail!("could not start");
}

async fn save_bosses<P: Persistence>(
    raid_handler: RaidHandler,
    persistence: P,
    interval: Duration,
    mut on_complete: impl FnMut(&P, Result<usize, P::Error>),
) {
    let mut interval = tokio::time::interval(interval);
    interval.tick().await; // The first tick completes immediately
    loop {
        interval.tick().await;
        let guard = raid_handler.bosses();
        let bosses = guard.iter().map(|entry| entry.boss()).collect::<Vec<_>>();

        let result = persistence.save_bosses(&bosses).await;
        on_complete(&persistence, result.map(|()| bosses.len()));
    }
}

// Loader order:
// 1. Try loading from Redis (if available)
// 2. Try loading from JSON file (if available)
// 3. Default to empty list
async fn get_initial_bosses(
    log: &slog::Logger,
    json_file: Option<&JsonFile>,
    redis_client: Option<&Redis>,
) -> anyhow::Result<Vec<Boss>> {
    async fn try_bosses_from_file(
        log: &slog::Logger,
        json_file: Option<&JsonFile>,
    ) -> Option<Vec<Boss>> {
        let json_file = json_file?;
        match json_file.get_bosses().await {
            Ok(bosses) => {
                slog::info!(
                    log, "Loaded bosses from JSON file";
                    "path" => json_file.path(), "count" => bosses.len()
                );
                Some(bosses)
            }
            Err(e) => {
                slog::warn!(
                    log, "Failed to load bosses from JSON file";
                    "error" => %e, "path" => json_file.path()
                );
                None
            }
        }
    }

    async fn try_bosses_from_redis(log: &slog::Logger, redis: Option<&Redis>) -> Option<Vec<Boss>> {
        let redis = redis?;
        match redis.get_bosses().await {
            Ok(bosses) => {
                slog::info!(log, "Loaded bosses from Redis"; "count" => bosses.len());
                Some(bosses)
            }
            Err(e) => {
                slog::warn!(log, "Failed to load bosses from Redis"; "error" => %e);
                None
            }
        }
    }

    let mut bosses = match try_bosses_from_redis(log, redis_client).await {
        Some(bosses) => bosses,
        None => try_bosses_from_file(log, json_file)
            .await
            .unwrap_or_else(|| {
                slog::info!(log, "Initializing empty boss list");
                Vec::new()
            }),
    };

    // See comment on `Boss::LVL_120_MEDUSA` for the reasoning
    if bosses
        .iter()
        .find(|b| b.name == Boss::LVL_120_MEDUSA.name)
        .is_none()
    {
        bosses.push(Boss::LVL_120_MEDUSA.clone());
    }

    Ok(bosses)
}
