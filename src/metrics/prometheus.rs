use crate::metrics::{LangMetric, Metric, MetricFactory, Metrics};
use crate::model::{LangString, Language};
use std::fmt;
use std::fmt::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

#[derive(Debug)]
pub struct PrometheusMetric {
    key: String,
    value: AtomicUsize,
}

impl Clone for PrometheusMetric {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            value: AtomicUsize::new(self.value.load(Relaxed)),
        }
    }
}

impl PrometheusMetric {
    pub fn new(key: String) -> Self {
        Self {
            key,
            value: AtomicUsize::new(0),
        }
    }
}

impl Metric for PrometheusMetric {
    fn get(&self) -> usize {
        self.value.load(Relaxed)
    }

    fn inc(&self) {
        self.value.fetch_add(1, Relaxed);
    }

    fn dec(&self) {
        self.value.fetch_sub(1, Relaxed);
    }

    fn set(&self, value: usize) {
        self.value.store(value, Relaxed);
    }
}

impl fmt::Display for PrometheusMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.key, self.value.load(Relaxed))
    }
}

#[derive(Debug)]
pub struct PrometheusMetricFactory {
    prefix: String,
    boss_tweets_counter_header: String,
    boss_subscriptions_gauge_header: String,
}

impl PrometheusMetricFactory {
    pub fn new(prefix: String) -> Self {
        let boss_tweets_counter_header = format!(
            "\
            # HELP {prefix}_tweets_total Number of tweets seen for boss\n\
            # TYPE {prefix}_tweets_total counter",
            prefix = &prefix
        );
        let boss_subscriptions_gauge_header = format!(
            "\
            # HELP {prefix}_subscriptions Number of subscriptions for boss\n\
            # TYPE {prefix}_subscriptions gauge",
            prefix = &prefix
        );
        Self {
            prefix,
            boss_tweets_counter_header,
            boss_subscriptions_gauge_header,
        }
    }
}

impl MetricFactory for PrometheusMetricFactory {
    type Output = String;
    type Metric = PrometheusMetric;

    fn boss_tweets_counter(&self, name: &LangString) -> LangMetric<PrometheusMetric> {
        let make = |lang: Language| {
            let key = format!(
                "{}_tweets_total{{name_ja=\"{}\",name_en=\"{}\",lang=\"{}\"}}",
                self.prefix,
                Label::new(name.ja.as_deref().unwrap_or("")),
                Label::new(name.en.as_deref().unwrap_or("")),
                lang.as_metric_label(),
            );

            PrometheusMetric::new(key)
        };

        LangMetric::new(make(Language::Japanese), make(Language::English))
    }

    fn boss_subscriptions_gauge(&self, name: &LangString) -> PrometheusMetric {
        let key = format!(
            "{}_subscriptions{{name_ja=\"{}\",name_en=\"{}\"}}",
            self.prefix,
            Label::new(name.ja.as_deref().unwrap_or("")),
            Label::new(name.en.as_deref().unwrap_or("")),
        );

        PrometheusMetric::new(key)
    }

    fn write(&self, metrics: &Metrics<'_, Self::Metric>) -> Self::Output {
        let mut out = String::new();

        writeln!(&mut out, "{}", self.boss_tweets_counter_header).unwrap();
        for metric in &metrics.boss_tweets_counters {
            metric.for_each(|m| writeln!(&mut out, "{}", m).unwrap());
        }

        writeln!(&mut out, "\n{}", self.boss_subscriptions_gauge_header).unwrap();
        for metric in &metrics.boss_subscriptions_gauges {
            writeln!(&mut out, "{}", metric).unwrap();
        }

        out
    }
}

struct Label<'a>(&'a str);
impl<'a> Label<'a> {
    pub fn new(value: &'a str) -> Self {
        Self(value)
    }
}
impl<'a> fmt::Display for Label<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Escape certain characters, per the Prometheus text-based format spec
        // https://prometheus.io/docs/instrumenting/exposition_formats/#comments-help-text-and-type-information
        for c in self.0.chars() {
            match c {
                '"' | '\\' | '\n' => write!(f, "{}", c.escape_default())?,
                _ => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use indoc::indoc;

    #[test]
    fn fmt_label() {
        assert_eq!(Label::new("normal").to_string(), "normal");
        assert_eq!(Label::new(r#"\"#).to_string(), r#"\\"#);
        assert_eq!(Label::new(r#"""#).to_string(), r#"\""#);
        assert_eq!(Label::new("\n").to_string(), r#"\n"#);

        // Unicode is not escaped
        assert_eq!(
            Label::new("Lv60 オオゾラッコ").to_string(),
            "Lv60 オオゾラッコ"
        );
    }

    #[test]
    fn fmt_metrics() {
        let factory = PrometheusMetricFactory::new("petronel".to_owned());
        let name = LangString {
            en: Some("Lvl 60 Ozorotter".into()),
            ja: Some("Lv60 オオゾラッコ".into()),
        };

        let counter = factory.boss_tweets_counter(&name);
        let gauge = factory.boss_subscriptions_gauge(&name);

        counter.get(Language::English).inc();
        counter.get(Language::English).inc();
        counter.get(Language::Japanese).set(35);
        gauge.set(100);

        let metrics = Metrics {
            boss_tweets_counters: vec![&counter],
            boss_subscriptions_gauges: vec![&gauge],
        };

        let output = factory.write(&metrics);
        let expected = indoc!(
            r#"
            # HELP petronel_tweets_total Number of tweets seen for boss
            # TYPE petronel_tweets_total counter
            petronel_tweets_total{name_ja="Lv60 オオゾラッコ",name_en="Lvl 60 Ozorotter",lang="ja"} 35
            petronel_tweets_total{name_ja="Lv60 オオゾラッコ",name_en="Lvl 60 Ozorotter",lang="en"} 2

            # HELP petronel_subscriptions Number of subscriptions for boss
            # TYPE petronel_subscriptions gauge
            petronel_subscriptions{name_ja="Lv60 オオゾラッコ",name_en="Lvl 60 Ozorotter"} 100
            "#
        );
        assert_eq!(output, expected);
    }
}
