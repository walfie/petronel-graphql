mod log;
mod opts;

use std::net::SocketAddr;

use chrono::Utc;
use futures::stream::StreamExt;
use petronel_graphql::image_hash::HyperImageHasher;
use petronel_graphql::model::Boss;
use petronel_graphql::{image_hash, twitter, RaidHandler};
use structopt::StructOpt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = opts::Options::from_args();

    let bind_addr: SocketAddr = format!("{}:{}", opt.bind_ip, opt.port).parse()?;

    let token = twitter::Token::new(
        opt.consumer_key,
        opt.consumer_secret,
        opt.access_token,
        opt.access_token_secret,
    );

    let log = log::logger(opt.json_logs);

    let conn = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(conn);

    let raid_handler = RaidHandler::new(
        get_initial_bosses().await?,
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
    tokio::spawn(hash_worker);

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
                    if boss.image_hash.is_none() && boss.image.canonical().is_some() {
                        hash_inbox.request_hash_for_boss(boss);
                    }

                    boss.last_seen_at.as_datetime() > long_ago
                });
            }
        }
    });

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

async fn get_initial_bosses() -> anyhow::Result<Vec<Boss>> {
    let mut bosses = Vec::<Boss>::new();

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
