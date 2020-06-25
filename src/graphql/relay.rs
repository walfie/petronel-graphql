use crate::model::Raid;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub trait Cursor: Serialize + DeserializeOwned {
    fn to_scalar_string(&self) -> String {
        let bytes = postcard::to_allocvec(self).expect("failed to stringify cursor");
        bs58::encode(&bytes).into_string()
    }

    fn from_scalar_string(value: &str) -> Option<Self> {
        let bytes = bs58::decode(value).into_vec().ok()?;
        postcard::from_bytes(&bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TweetCursor {
    pub created_at_millis: i64,
}

impl Cursor for TweetCursor {}

impl From<&Raid> for TweetCursor {
    fn from(raid: &Raid) -> Self {
        Self {
            created_at_millis: raid.created_at.as_datetime().timestamp_millis(),
        }
    }
}

#[juniper::graphql_scalar]
impl<S> GraphQLScalar for TweetCursor
where
    S: juniper::ScalarValue,
{
    fn resolve(&self) -> juniper::Value {
        juniper::Value::scalar(self.to_scalar_string())
    }

    fn from_input_value(value: &juniper::InputValue) -> Option<Self> {
        let input = value.as_string_value()?;
        Self::from_scalar_string(input)
    }

    fn from_str<'a>(value: juniper::ScalarToken<'a>) -> juniper::ParseScalarResult<'a, S> {
        <String as juniper::ParseScalarValue<S>>::from_str(value)
    }
}
