mod log;
mod opts;

use std::net::SocketAddr;

use chrono::Utc;
use futures::stream::StreamExt;
use petronel_graphql::image_hash::HyperImageHasher;
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

    let concurrency = 5; // TODO: configurable
    let raid_handler = RaidHandler::new(opt.raid_history_size, opt.broadcast_capacity);

    // Cleanup task
    tokio::spawn({
        let max_age = chrono::Duration::from_std(opt.boss_max_age)?;
        let raid_handler = raid_handler.clone();
        let mut interval = tokio::time::interval(opt.cleanup_interval);
        async move {
            loop {
                interval.tick().await;
                raid_handler.retain(Utc::now() - max_age);
            }
        }
    });

    // Fetch boss images and calculate image hashes
    let hash_updater = image_hash::Updater::new(
        log.clone(),
        HyperImageHasher::new(client.clone()),
        raid_handler.clone(),
        concurrency,
    );
    tokio::spawn(hash_updater.run());

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
