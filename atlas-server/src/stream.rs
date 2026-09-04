//! Tier-1 ingestion: the live event feed the reconciliation loop runs
//! alongside its full scans (`docs/change_monitoring_design.md` §5).
//!
//! A [`Source`] is a thing that yields normalized
//! [`ChangeEvent`](atlas_lib::atlas::event::ChangeEvent)s whenever the cloud has
//! something to say. The poll loop selects over it and the reconciliation
//! ticker, so the two tiers share one writer and mutations of the live graph
//! stay serialized without a lock dance.
//!
//! [`Source::Disabled`] is not an error state — it is the normal shape for a
//! provider with no feed wired up, and for the whole server when the operator
//! has not created a queue. Its `next` never resolves, so the `select!` branch
//! simply never fires and the graph converges on Tier 3 alone.

use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::event::ChangeEvent;
use atlas_lib::cloud::amazon::events::stream::EventQueue;
use std::time::Duration;

/// One drain of a feed. Infallible like the provider scans: a source that could
/// not be read returns no events plus a report saying why, so a dead feed can
/// never pass for a quiet one.
pub struct Batch {
    pub events: Vec<ChangeEvent>,
    pub report: CollectionReport,
}

pub enum Source {
    /// No Tier-1 feed configured. Tier 3 alone keeps the graph correct, just
    /// with poll-interval latency.
    Disabled,
    /// AWS EventBridge events delivered to an SQS queue.
    Aws(Box<EventQueue>),
}

impl Source {
    /// Build the AWS source for a queue URL, resolving the region's SDK config
    /// the same way the collectors do.
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

    /// Wait `delay`, then drain the next batch of changes.
    ///
    /// The delay is the caller's [`Backoff`] and belongs *inside* this future
    /// on purpose: sleeping in the poll loop's body instead would stall the
    /// reconciliation tick it is selected against, so a broken event feed would
    /// slow down the tier that still works.
    ///
    /// This future must not be dropped once polled. A drain deletes the
    /// messages it processed *before* handing them back, so a future abandoned
    /// in that window loses events outright — SQS has already forgotten them.
    /// `poll::run` therefore keeps it pinned across loop iterations rather than
    /// recreating it each time round the `select!`.
    pub async fn next(&self, delay: Duration) -> Batch {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match self {
            // Never resolves: the caller's `select!` arm is simply inert.
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

/// How long to wait before touching a failing feed again.
///
/// Without this the loop spins: a receive that fails — a 403 on the queue
/// policy, a wrong URL, an outage — returns *immediately*, and the `select!`
/// starts another one at once. The long poll only paces the success path, so a
/// permissions typo would otherwise mean SQS requests as fast as the network
/// allows, one warning logged per iteration, for as long as the server runs.
///
/// Recovery is immediate rather than gradual: one good drain clears the delay
/// entirely, because the next event is worth having as soon as the feed is
/// healthy again.
pub struct Backoff {
    current: Duration,
}

impl Backoff {
    /// First wait after a failure. Short — most feed failures are transient and
    /// a second of latency is not worth avoiding a retry over.
    const BASE: Duration = Duration::from_secs(1);

    /// Ceiling on the wait. At a minute, a feed that is down for hours costs a
    /// negligible number of requests while still recovering promptly.
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

    /// The spin this exists to stop: a feed failing every time must not be
    /// retried without pause, and must not creep past the ceiling either.
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
