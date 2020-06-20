mod schema;

use crate::raid_handler::RaidHandler;
use futures::FutureExt;
use juniper::{EmptyMutation, RootNode};
use juniper_subscriptions::Coordinator;
use juniper_warp::subscriptions::graphql_subscriptions;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use warp::{http::Response, Filter};

type Schema = RootNode<'static, schema::Query, EmptyMutation<RaidHandler>, schema::Subscription>;

fn schema() -> Schema {
    Schema::new(
        schema::Query,
        EmptyMutation::<RaidHandler>::new(),
        schema::Subscription,
    )
}

pub fn routes(handler: RaidHandler) -> impl Filter<Extract = impl warp::Reply> + Clone {
    let graphql_context = {
        let handler = handler.clone();
        warp::any().map(move || handler.clone())
    };

    let coordinator = Arc::new(juniper_subscriptions::Coordinator::new(schema()));
    let websocket_graphql = warp::path!("graphql")
        .and(warp::ws())
        .and(graphql_context.clone())
        .and(warp::any().map(move || coordinator.clone()))
        .map(
            |ws: warp::ws::Ws,
             ctx: RaidHandler,
             coordinator: Arc<Coordinator<'static, _, _, _, _, _>>| {
                ws.on_upgrade(|websocket| -> Pin<Box<dyn Future<Output = ()> + Send>> {
                    graphql_subscriptions(websocket, coordinator, ctx)
                        .map(|r| {
                            if let Err(e) = r {
                                // TODO
                                println!("Websocket error: {}", e);
                            }
                        })
                        .boxed()
                })
            },
        )
        .map(|reply| warp::reply::with_header(reply, "Sec-WebSocket-Protocol", "graphql-ws"));

    let post_graphql = warp::path!("graphql")
        .and(warp::header::exact_ignore_case(
            "accept",
            "application/json",
        ))
        .and(juniper_warp::make_graphql_filter(
            schema(),
            graphql_context.boxed(),
        ));

    // TODO: Configurable
    let get_graphiql = warp::path!("graphiql").and(warp::get()).map(|| {
        Response::builder()
            .header("content-type", "text/html")
            .body(include_str!("graphiql.html"))
    });

    // TODO: Configurable
    let get_metrics = warp::path!("metrics").and(warp::get()).map(move || {
        Response::builder()
            .header("content-type", "text/plain; version=0.0.4")
            .body(handler.metrics())
    });

    // TODO: Configurable
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
        .allow_header("*")
        .max_age(86400);

    let routes = post_graphql
        .or(websocket_graphql)
        .or(get_graphiql)
        .or(get_metrics)
        .with(cors);

    routes
}
