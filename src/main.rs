mod log;
mod opts;

use std::net::SocketAddr;

use chrono::Utc;
use futures::stream::StreamExt;
use petronel_graphql::image_hash::HyperImageHasher;
use petronel_graphql::model::Boss;
use petronel_graphql::persistence::{JsonFile, Persistence};
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
    let initial_bosses = get_initial_bosses(&log, &opt.clone()).await?;
    let bosses_to_request_hashes_for = initial_bosses
        .iter()
        .filter(|b| b.needs_image_hash_update())
        .cloned()
        .collect::<Vec<_>>();

    let raid_handler = RaidHandler::new(
        initial_bosses,
        opt.raid_history_size,
        opt.broadcast_capacity,
    );

    let token = twitter::Token::new(
        opt.consumer_key,
        opt.consumer_secret,
        opt.access_token,
        opt.access_token_secret,
    );

    // Fetch boss images and calculate image hashes
    let hash_updater = image_hash::Updater::new(
        log.clone(),
        HyperImageHasher::new(client.clone()),
        raid_handler.clone(),
        opt.image_hash_concurrency,
    );
    let (hash_inbox, hash_worker) = hash_updater.run();
    tokio::spawn(hash_worker);

    bosses_to_request_hashes_for
        .iter()
        .for_each(|boss| hash_inbox.request_hash_for_boss(boss));

    // Cleanup task that periodically:
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

    // Periodically write boss data to persistent storage
    if let Some(path) = opt.storage_file_path {
        let file = JsonFile::new(path);

        tokio::spawn({
            let log = log.clone();
            let mut interval = tokio::time::interval(opt.storage_file_flush_interval);
            let raid_handler = raid_handler.clone();

            async move {
                loop {
                    interval.tick().await;
                    let guard = raid_handler.bosses();
                    let bosses = guard.iter().map(|entry| entry.boss()).collect::<Vec<_>>();
                    if let Err(e) = file.save_bosses(&bosses).await {
                        slog::warn!(
                            log, "Failed to write boss data to file";
                            "error" => %e, "filename" => file.filename()
                        )
                    }
                }
            }
        });
    }

    let (mut tweet_stream, worker) = twitter::connect_with_retries(
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

    slog::info!(log, "Starting HTTP server"; "port" => opt.port, "ip" => &opt.bind_ip);
    let server = warp::serve(routes).try_bind(bind_addr);

    tokio::select! {
        _ = worker => {
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

async fn get_initial_bosses(log: &slog::Logger, opt: &opts::Options) -> anyhow::Result<Vec<Boss>> {
    let mut bosses = if let Some(path) = &opt.storage_file_path {
        let file = JsonFile::new(path.clone());
        match file.get_bosses().await {
            Ok(bosses) => {
                slog::info!(
                    log, "Loaded bosses from file";
                    "filename" => file.filename(), "count" => bosses.len()
                );
                bosses
            }
            Err(e) => {
                slog::warn!(
                    log, "Failed to load bosses from file. Initializing with empty list.";
                    "error" => %e, "filename" => file.filename()
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
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
