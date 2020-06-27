use crate::model::{Raid, TweetId};
use juniper::{FieldResult, IntoFieldResult};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

#[derive(Debug, Clone, PartialEq, juniper::GraphQLObject)]
pub struct PageInfo {
    pub has_previous_page: bool,
    pub has_next_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

pub trait Cursor: Serialize + DeserializeOwned {
    type Edge;

    fn to_scalar_string(&self) -> String {
        let bytes = postcard::to_allocvec(self).expect("failed to stringify cursor");
        bs58::encode(&bytes).into_string()
    }

    fn from_scalar_string(value: &str) -> Option<Self> {
        let bytes = bs58::decode(value).into_vec().ok()?;
        postcard::from_bytes(&bytes).ok()
    }

    fn from_edge(edge: &Self::Edge) -> Self;
    fn matches_edge(&self, edge: &Self::Edge) -> bool;

    fn paginate<E, I, F, Out>(
        mut edges: I,
        total_edges_length: usize,
        map_fn: F,
        first: Option<i32>,
        after: Option<Self>,
        last: Option<i32>,
        before: Option<Self>,
    ) -> FieldResult<(Vec<Out>, PageInfo)>
    where
        I: Iterator<Item = E>,
        E: AsRef<Self::Edge>,
        F: Fn(E) -> Out,
        Out: Borrow<Self::Edge>,
    {
        enum FirstOrLast {
            First(usize),
            Last(usize),
        }

        let first_or_last = match (first, last) {
            (None, None) => return Err("Either `first` or `last` must be specified").into_result(),
            (Some(_), Some(_)) => {
                return Err("Only one of `first` or `last` should be specified").into_result()
            }
            (Some(f), None) if f >= 0 => FirstOrLast::First(f as usize),
            (None, Some(l)) if l >= 0 => FirstOrLast::Last(l as usize),
            _ => return Err("`first` and `last` must be non-negative").into_result(),
        };

        let mut skipped = 0;

        // If `after` is specified, skip until we see the cursor
        // TODO: The pagination algorithm in the graphql spec states that if the cursor doesn't
        // exist, don't slice the edges
        if let Some(cursor) = after {
            while let Some(edge) = edges.next() {
                skipped += 1;
                if cursor.matches_edge(edge.as_ref()) {
                    break;
                }
            }
        }

        let remaining_edges = total_edges_length - skipped;

        let output_edges = match (first_or_last, before) {
            (FirstOrLast::First(count), None) => {
                edges.take(count).map(map_fn).collect::<Vec<Out>>()
            }
            (FirstOrLast::First(count), Some(before)) => {
                // If `before` is specified, take until we see the cursor
                let mut out = Vec::with_capacity(count);
                let mut taken = 0;

                while let Some(edge) = edges.next() {
                    if taken < count && !before.matches_edge(edge.as_ref()) {
                        out.push(map_fn(edge));
                        taken += 1;
                    } else {
                        break;
                    }
                }
                out
            }
            (FirstOrLast::Last(count), None) => {
                let skip_n = if count > remaining_edges {
                    0
                } else {
                    remaining_edges - count
                };

                skipped += skip_n;

                edges.skip(skip_n).map(map_fn).collect::<Vec<Out>>()
            }
            (FirstOrLast::Last(count), Some(before)) => {
                // This is highly inefficient. TODO: Maybe use reversible iterator
                let mut out = edges
                    .take_while(|edge| !before.matches_edge(edge.as_ref()))
                    .map(map_fn)
                    .collect::<Vec<Out>>();

                if out.len() <= count {
                    out
                } else {
                    let skip_n = out.len() - count;
                    skipped += skip_n;

                    out.split_off(skip_n)
                }
            }
        };

        let to_cursor = |edge: &Out| Self::from_edge(edge.borrow()).to_scalar_string();

        let page_info = PageInfo {
            has_previous_page: skipped > 0,
            has_next_page: output_edges.len() + skipped < total_edges_length,
            start_cursor: output_edges.first().map(to_cursor),
            end_cursor: output_edges.last().map(to_cursor),
        };

        Ok((output_edges, page_info))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TweetCursor {
    pub tweet_id: TweetId,
}

impl Cursor for TweetCursor {
    type Edge = Raid;

    fn from_edge(edge: &Self::Edge) -> Self {
        Self {
            tweet_id: edge.tweet_id,
        }
    }

    fn matches_edge(&self, edge: &Self::Edge) -> bool {
        self.tweet_id == edge.tweet_id
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

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct TestCursor(usize);

    impl Cursor for TestCursor {
        type Edge = usize;

        fn from_edge(edge: &Self::Edge) -> Self {
            Self(edge.clone())
        }

        fn matches_edge(&self, edge: &Self::Edge) -> bool {
            self.0 == *edge
        }
    }

    struct TestCase {
        first: Option<i32>,
        after: Option<TestCursor>,
        last: Option<i32>,
        before: Option<TestCursor>,
    }

    impl TestCase {
        fn run(&self) -> FieldResult<(Vec<usize>, PageInfo)> {
            let all_edges = (0..=100).map(Arc::new).collect::<Vec<Arc<usize>>>();

            let output = TestCursor::paginate(
                all_edges.iter(),
                all_edges.len(),
                |arc| **arc,
                self.first,
                self.after.clone(),
                self.last,
                self.before.clone(),
            );

            output
        }
    }

    #[test]
    fn pagination() {
        // Pagination with `first` only, when requesting more items than exist
        assert_eq!(
            TestCase {
                first: Some(200),
                after: None,
                last: None,
                before: None,
            }
            .run()
            .unwrap(),
            (
                (0..=100).collect(),
                PageInfo {
                    has_previous_page: false,
                    has_next_page: false,
                    start_cursor: Some(TestCursor(0).to_scalar_string()),
                    end_cursor: Some(TestCursor(100).to_scalar_string()),
                }
            )
        );

        // Pagination with `first` only
        assert_eq!(
            TestCase {
                first: Some(10),
                after: None,
                last: None,
                before: None,
            }
            .run()
            .unwrap(),
            (
                (0..=9).collect(),
                PageInfo {
                    has_previous_page: false,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(0).to_scalar_string()),
                    end_cursor: Some(TestCursor(9).to_scalar_string()),
                }
            )
        );

        // Pagination with `first` and `after`
        assert_eq!(
            TestCase {
                first: Some(10),
                after: Some(TestCursor(50)),
                last: None,
                before: None,
            }
            .run()
            .unwrap(),
            (
                (51..=60).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(51).to_scalar_string()),
                    end_cursor: Some(TestCursor(60).to_scalar_string()),
                }
            )
        );

        // Pagination with `first` and `after`, when not enough items at the end
        assert_eq!(
            TestCase {
                first: Some(10),
                after: Some(TestCursor(95)),
                last: None,
                before: None,
            }
            .run()
            .unwrap(),
            (
                (96..=100).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: false,
                    start_cursor: Some(TestCursor(96).to_scalar_string()),
                    end_cursor: Some(TestCursor(100).to_scalar_string()),
                }
            )
        );

        // Pagination with `first` and `after`, but the `after` cursor doesn't exist
        assert_eq!(
            TestCase {
                first: Some(10),
                after: Some(TestCursor(43253)),
                last: None,
                before: None,
            }
            .run()
            .unwrap(),
            (
                vec![],
                PageInfo {
                    has_previous_page: true,
                    has_next_page: false,
                    start_cursor: None,
                    end_cursor: None,
                }
            )
        );

        // Pagination with `last` only, when requesting more items than exist
        assert_eq!(
            TestCase {
                first: None,
                after: None,
                last: Some(200),
                before: None,
            }
            .run()
            .unwrap(),
            (
                (0..=100).collect(),
                PageInfo {
                    has_previous_page: false,
                    has_next_page: false,
                    start_cursor: Some(TestCursor(0).to_scalar_string()),
                    end_cursor: Some(TestCursor(100).to_scalar_string()),
                }
            )
        );

        // Pagination with `last` only
        assert_eq!(
            TestCase {
                first: None,
                after: None,
                last: Some(10),
                before: None,
            }
            .run()
            .unwrap(),
            (
                (91..=100).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: false,
                    start_cursor: Some(TestCursor(91).to_scalar_string()),
                    end_cursor: Some(TestCursor(100).to_scalar_string()),
                }
            )
        );

        // Pagination with `last` and `before`
        assert_eq!(
            TestCase {
                first: None,
                after: None,
                last: Some(10),
                before: Some(TestCursor(50)),
            }
            .run()
            .unwrap(),
            (
                (40..=49).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(40).to_scalar_string()),
                    end_cursor: Some(TestCursor(49).to_scalar_string()),
                }
            )
        );

        // Pagination with `last` and `before`, when not enough items at the beginning
        assert_eq!(
            TestCase {
                first: None,
                after: None,
                last: Some(10),
                before: Some(TestCursor(5)),
            }
            .run()
            .unwrap(),
            (
                (0..=4).collect(),
                PageInfo {
                    has_previous_page: false,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(0).to_scalar_string()),
                    end_cursor: Some(TestCursor(4).to_scalar_string()),
                }
            )
        );

        // Pagination with `last` and `before`, but the `before` cursor doesn't exist
        assert_eq!(
            TestCase {
                first: None,
                after: None,
                last: Some(10),
                before: Some(TestCursor(3532)),
            }
            .run()
            .unwrap(),
            (
                (91..=100).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: false,
                    start_cursor: Some(TestCursor(91).to_scalar_string()),
                    end_cursor: Some(TestCursor(100).to_scalar_string()),
                }
            )
        );

        // Pagination with `first`, `after`, and `before`
        assert_eq!(
            TestCase {
                first: Some(10),
                after: Some(TestCursor(40)),
                last: None,
                before: Some(TestCursor(60)),
            }
            .run()
            .unwrap(),
            (
                (41..=50).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(41).to_scalar_string()),
                    end_cursor: Some(TestCursor(50).to_scalar_string()),
                }
            )
        );

        // Pagination with `last`, `after`, and `before`
        assert_eq!(
            TestCase {
                first: None,
                after: Some(TestCursor(40)),
                last: Some(10),
                before: Some(TestCursor(60)),
            }
            .run()
            .unwrap(),
            (
                (50..=59).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(50).to_scalar_string()),
                    end_cursor: Some(TestCursor(59).to_scalar_string()),
                }
            )
        );

        // Pagination with `first`, `after`, and `before`, with a large `first`
        assert_eq!(
            TestCase {
                first: Some(50),
                after: Some(TestCursor(40)),
                last: None,
                before: Some(TestCursor(60)),
            }
            .run()
            .unwrap(),
            (
                (41..=59).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(41).to_scalar_string()),
                    end_cursor: Some(TestCursor(59).to_scalar_string()),
                }
            )
        );

        // Pagination with `last`, `after`, and `before`, with a large `last`
        assert_eq!(
            TestCase {
                first: None,
                after: Some(TestCursor(40)),
                last: Some(50),
                before: Some(TestCursor(60)),
            }
            .run()
            .unwrap(),
            (
                (41..=59).collect(),
                PageInfo {
                    has_previous_page: true,
                    has_next_page: true,
                    start_cursor: Some(TestCursor(41).to_scalar_string()),
                    end_cursor: Some(TestCursor(59).to_scalar_string()),
                }
            )
        );
    }
}
