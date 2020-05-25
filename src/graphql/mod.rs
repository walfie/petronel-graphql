mod schema;

use crate::raid_handler::RaidHandler;
use async_graphql::http::GQLResponse;
use async_graphql::{EmptyMutation, QueryBuilder, Schema};
use async_graphql_warp::BadRequest;
use http::StatusCode;
use std::convert::Infallible;
use warp::{http::Response, Filter, Rejection, Reply};

pub fn routes(
    handler: RaidHandler,
) -> impl Filter<Extract = impl warp::Reply, Error = Infallible> + Clone {
    let schema = Schema::build(schema::QueryRoot, EmptyMutation, schema::SubscriptionRoot)
        .data(handler.clone())
        .finish();

    let post_graphql = warp::path!("graphql")
        .and(warp::header::exact_ignore_case(
            "accept",
            "application/json",
        ))
        .and(async_graphql_warp::graphql(schema.clone()))
        .and_then(|(schema, builder): (_, QueryBuilder)| async move {
            let resp = builder.execute(&schema).await;
            Ok::<_, Infallible>(warp::reply::json(&GQLResponse(resp)).into_response())
        });

    let websocket_graphql = warp::path!("graphql")
        .and(warp::header::exact_ignore_case("connection", "upgrade"))
        .and(async_graphql_warp::graphql_subscription(schema));

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
        .with(cors)
        .recover(|err: Rejection| async move {
            if let Some(BadRequest(err)) = err.find() {
                return Ok::<_, Infallible>(warp::reply::with_status(
                    err.to_string(),
                    StatusCode::BAD_REQUEST,
                ));
            }

            // TODO: It's not always a 500. Could be 404, etc
            Ok(warp::reply::with_status(
                "INTERNAL_SERVER_ERROR".to_string(),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        });

    routes
}
