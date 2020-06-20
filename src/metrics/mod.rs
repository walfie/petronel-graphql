mod prometheus;

pub use crate::metrics::prometheus::{PrometheusMetric, PrometheusMetricFactory};
use crate::model::{LangString, Language};

pub trait Metric: Clone {
    fn get(&self) -> usize;
    fn inc(&self);
    fn dec(&self);
    fn set(&self, value: usize);
}

pub struct PerBossMetrics<'m, M: Metric> {
    pub boss_tweets_counters: Vec<&'m LangMetric<M>>,
    pub boss_subscriptions_gauges: Vec<&'m M>,
}

pub trait MetricFactory {
    type Metric: Metric;
    type Output;

    fn boss_tweets_counter(&self, name: &LangString) -> LangMetric<Self::Metric>;
    fn boss_subscriptions_gauge(&self, name: &LangString) -> Self::Metric;

    fn write_per_boss_metrics(&self, metrics: &PerBossMetrics<'_, Self::Metric>) -> Self::Output;
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
