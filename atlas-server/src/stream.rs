use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::event::ChangeEvent;
use atlas_lib::atlas::flow::FlowObservation;
use atlas_lib::cloud::amazon::events::stream::EventQueue;
use atlas_lib::cloud::amazon::flow_logs::stream::FlowLogQueue;
use std::time::Duration;

pub struct Batch {
    pub events: Vec<ChangeEvent>,
    pub report: CollectionReport,
}

pub enum Source {
    Disabled,
    Aws(Box<EventQueue>),
}

impl Source {
    pub async fn aws(region: &str, queue_url: &str, exclude_by_default: bool) -> Self {
        let config = atlas_lib::cloud::amazon::load_config(region).await;
        Source::Aws(Box::new(EventQueue::new(
            &config,
            queue_url,
            region,
            exclude_by_default,
        )))
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, Source::Disabled)
    }

    pub async fn next(&self, delay: Duration) -> Batch {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match self {
            Source::Disabled => std::future::pending().await,
            Source::Aws(queue) => {
                let batch = queue.receive().await;
                Batch {
                    events: batch.events,
                    report: batch.report,
                }
            }
        }
    }
}

pub struct FlowBatch {
    pub observations: Vec<FlowObservation>,
    pub report: CollectionReport,
}

pub enum FlowSource {
    Disabled,
    Aws(Box<FlowLogQueue>),
}

impl FlowSource {
    pub async fn aws(region: &str, queue_url: &str) -> Self {
        let config = atlas_lib::cloud::amazon::load_config(region).await;
        FlowSource::Aws(Box::new(FlowLogQueue::new(&config, queue_url, region)))
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, FlowSource::Disabled)
    }

    pub async fn next(&self, delay: Duration) -> FlowBatch {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match self {
            FlowSource::Disabled => std::future::pending().await,
            FlowSource::Aws(queue) => {
                let batch = queue.receive().await;
                FlowBatch {
                    observations: batch.observations,
                    report: batch.report,
                }
            }
        }
    }
}

pub struct Backoff {
    current: Duration,
}

impl Backoff {
    const BASE: Duration = Duration::from_secs(1);

    const MAX: Duration = Duration::from_secs(60);

    pub fn new() -> Self {
        Self {
            current: Duration::ZERO,
        }
    }

    pub fn delay(&self) -> Duration {
        self.current
    }

    pub fn record(&mut self, healthy: bool) {
        self.current = match (healthy, self.current) {
            (true, _) => Duration::ZERO,
            (false, Duration::ZERO) => Self::BASE,
            (false, current) => (current * 2).min(Self::MAX),
        };
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failures_back_off_up_to_the_ceiling() {
        let mut backoff = Backoff::new();
        assert_eq!(backoff.delay(), Duration::ZERO, "healthy feeds never wait");

        backoff.record(false);
        assert_eq!(backoff.delay(), Backoff::BASE);

        let mut previous = backoff.delay();
        for _ in 0..20 {
            backoff.record(false);
            assert!(backoff.delay() >= previous);
            assert!(backoff.delay() <= Backoff::MAX);
            previous = backoff.delay();
        }
        assert_eq!(previous, Backoff::MAX);
    }

    #[test]
    fn one_good_drain_clears_the_delay() {
        let mut backoff = Backoff::new();
        backoff.record(false);
        backoff.record(false);
        assert!(backoff.delay() > Duration::ZERO);

        backoff.record(true);

        assert_eq!(backoff.delay(), Duration::ZERO);
    }
}
