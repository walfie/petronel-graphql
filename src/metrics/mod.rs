mod prometheus;

pub use crate::metrics::prometheus::{PrometheusMetric, PrometheusMetricFactory};
use crate::model::{LangString, Language};

pub trait Metric: Clone {
    fn get(&self) -> usize;
    fn inc(&self);
    fn dec(&self);
    fn set(&self, value: usize);
}

pub struct Metrics<'m, M: Metric> {
    pub boss_tweet_counters: Vec<&'m LangMetric<M>>,
    pub boss_subscriber_gauges: Vec<&'m M>,
}

pub trait MetricFactory {
    type Metric: Metric;
    type Output;

    fn boss_tweet_counter(&self, name: &LangString) -> LangMetric<Self::Metric>;
    fn boss_subscriber_gauge(&self, name: &LangString) -> Self::Metric;
    fn write(&self, metrics: &Metrics<'_, Self::Metric>) -> Self::Output;
}

#[derive(Debug, Clone)]
pub struct LangMetric<M> {
    ja: M,
    en: M,
}

impl<M> LangMetric<M>
where
    M: Metric,
{
    pub fn new(ja: M, en: M) -> Self {
        Self { ja, en }
    }

    pub fn get(&self, lang: Language) -> &M {
        match lang {
            Language::Japanese => &self.ja,
            Language::English => &self.en,
        }
    }

    pub fn for_each(&self, mut f: impl FnMut(&M)) {
        for metric in &[&self.ja, &self.en] {
            f(metric)
        }
    }
}
