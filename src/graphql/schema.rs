use std::borrow::Cow;
use std::pin::Pin;
use std::str;
use std::sync::Arc;

use crate::model::*;
use crate::raid_handler::{BossEntry, RaidHandler};

use futures::future::ready;
use futures::stream::Stream;
use juniper::{
    Arguments, BoxFuture, DefaultScalarValue, ExecutionResult, Executor, GraphQLType, Selection,
};

#[derive(juniper::GraphQLScalarValue)]
#[graphql(transparent, name = "ID")]
pub struct Id(String);

#[derive(juniper::GraphQLScalarValue)]
#[graphql(transparent, name = "DateTime")]
/// An ISO-8601 encoded UTC date string.
pub struct DateTime(String);

pub struct Query;

impl juniper::Context for RaidHandler {}

fn get_node(raid_handler: &RaidHandler, id: &str) -> Option<Node> {
    match id.parse().ok()? {
        NodeId::Boss(name) => raid_handler.boss(&name).map(Node::Boss),
        NodeId::Tweet { boss_name, id } => raid_handler.boss(&boss_name).and_then(|boss| {
            boss.history()
                .read()
                .iter()
                .find(|tweet| tweet.tweet_id == id)
                .map(|t| Node::Tweet(t.clone()))
        }),
    }
}

#[juniper::graphql_object(Context = RaidHandler)]
impl Query {
    fn node(&self, ctx: &RaidHandler, id: Id) -> Option<Node> {
        get_node(ctx, &id.0)
    }

    fn nodes(&self, ctx: &RaidHandler, ids: Vec<Id>) -> Vec<Option<Node>> {
        // TODO: Could be optimized more for tweets. The IDs requested could be multiple tweets
        // from the same boss, but we currently iterate through the list once for each requested
        // tweet node, when instead we could iterate once per unique boss.
        ids.iter().map(|id| get_node(ctx, &id.0)).collect()
    }

    // TODO: pagination, first/last, etc
    fn bosses(&self, ctx: &RaidHandler) -> Vec<Arc<BossEntry>> {
        ctx.bosses().clone()
    }

    fn boss(&self, ctx: &RaidHandler, name: String) -> Option<Arc<BossEntry>> {
        ctx.boss(&name.into())
    }
}

pub struct Subscription;
type SubscriptionStream<T> = Pin<Box<dyn Stream<Item = T> + Send>>;

#[juniper::graphql_subscription(Context = RaidHandler)]
impl Subscription {
    async fn bosses(&self, ctx: &RaidHandler) -> SubscriptionStream<Arc<BossEntry>> {
        Box::pin(ctx.subscribe_boss_updates())
    }

    async fn tweets(&self, ctx: &RaidHandler, boss_name: String) -> SubscriptionStream<Arc<Raid>> {
        Box::pin(ctx.subscribe(boss_name.into()))
    }
}

#[juniper::graphql_object]
/// A string (name, URL, etc) that differs based on language
impl LangString {
    /// The Japanese string, if it exists. Otherwise, the English one.
    fn canonical(&self) -> Option<&str> {
        self.ja.as_deref().or_else(|| self.en.as_deref())
    }

    /// Japanese string
    fn ja(&self) -> Option<&str> {
        self.ja.as_deref()
    }

    /// English string
    fn en(&self) -> Option<&str> {
        self.en.as_deref()
    }
}

#[juniper::graphql_object(name = "Boss", interfaces = [Node])]
/// A raid boss
impl BossEntry {
    /// Node ID
    fn id(&self) -> Id {
        Id(self.node_id().to_string())
    }

    /// Boss name
    fn name(&self) -> &LangString {
        &self.boss().name
    }

    /// Twitter image URL
    fn image(&self) -> &LangString {
        &self.boss().image
    }

    /// The level of the boss, if known
    fn level(&self) -> Option<i32> {
        self.boss().level.map(|level| level as i32)
    }

    /// List of raid tweets
    fn tweets(&self) -> Vec<Arc<Raid>> {
        // TODO: Pagination
        self.history().read().iter().cloned().collect()
    }
}

fn raid_node_id(raid: &Raid) -> Id {
    let node_id = NodeId::Tweet {
        id: raid.tweet_id,
        boss_name: Cow::Borrowed(&raid.boss_name),
    };

    Id(node_id.to_string())
}

#[juniper::graphql_object(name = "Tweet", interfaces = [Node])]
/// A tweet containing a raid invite
impl Raid {
    /// Node ID
    fn id(&self) -> Id {
        raid_node_id(self)
    }

    /// Raid ID
    fn raid_id(&self) -> &str {
        &self.id
    }

    /// Tweet ID
    fn tweet_id(&self) -> Id {
        Id(self.tweet_id.to_string())
    }

    /// Additional text associated with the tweet
    fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Tweet creation date
    fn created_at(&self) -> DateTime {
        DateTime(self.created_at.as_str().to_owned())
    }

    /// Twitter username
    fn username(&self) -> &str {
        &self.user_name
    }

    /// Twitter user icon
    #[graphql(name = "icon")]
    fn user_image(&self) -> Option<&str> {
        self.user_image.as_deref()
    }
}

enum Node {
    Boss(Arc<BossEntry>),
    Tweet(Arc<Raid>),
}

juniper::graphql_interface!(Node: () |&self| {
    field id() -> Id {
        match self {
            Node::Boss(boss) => Id(boss.node_id().to_string()),
            Node::Tweet(tweet) => raid_node_id(&tweet),
        }
    }

    instance_resolvers: |_| {
        &BossEntry => match *self { Node::Boss(ref b) => Some(b.as_ref()), _ => None },
        &Raid => match *self { Node::Tweet(ref t) => Some(t.as_ref()), _ => None },
    }
});

impl juniper::GraphQLTypeAsync<DefaultScalarValue> for Node {
    fn resolve_field_async<'a>(
        &'a self,
        info: &'a Self::TypeInfo,
        field_name: &'a str,
        arguments: &'a Arguments<DefaultScalarValue>,
        executor: &'a Executor<Self::Context, DefaultScalarValue>,
    ) -> BoxFuture<'a, ExecutionResult<DefaultScalarValue>> {
        Box::pin(ready(GraphQLType::resolve_field(
            self, info, field_name, arguments, executor,
        )))
    }

    fn resolve_async<'a>(
        &'a self,
        info: &'a Self::TypeInfo,
        selection_set: Option<&'a [Selection<DefaultScalarValue>]>,
        executor: &'a Executor<Self::Context, DefaultScalarValue>,
    ) -> BoxFuture<'a, ExecutionResult<DefaultScalarValue>> {
        Box::pin(ready(GraphQLType::resolve(
            self,
            info,
            selection_set,
            executor,
        )))
    }

    fn resolve_into_type_async<'a>(
        &'a self,
        info: &'a Self::TypeInfo,
        type_name: &str,
        selection_set: Option<&'a [Selection<'a, DefaultScalarValue>]>,
        executor: &'a Executor<'a, 'a, Self::Context, DefaultScalarValue>,
    ) -> BoxFuture<'a, ExecutionResult<DefaultScalarValue>> {
        Box::pin(ready(GraphQLType::resolve_into_type(
            self,
            info,
            type_name,
            selection_set,
            executor,
        )))
    }
}
