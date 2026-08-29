//! The shared fan-out for providers that read many independent APIs per scope
//! (an AWS region, a GCP project).
//!
//! Every collector travels with its own name, so a failure can be attributed to
//! the exact API that failed instead of sinking the whole provider. Running
//! them through here is what keeps [`crate::atlas::collection`]'s contract
//! honest: the only way to consume a collector's `Result` is to hand it to
//! `run_all`, which records the error rather than dropping it.

use crate::atlas::collection::{CollectionReport, CollectionSource};
use std::future::Future;
use std::pin::Pin;

pub type NamedCollector<'a, T> = (
    &'static str,
    Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error>>> + 'a>>,
);

/// Run every collector concurrently and split the outcomes: what was read goes
/// into the returned collections, what failed is recorded against
/// `{scope}/{name}` in the returned report.
pub async fn run_all<T>(
    collectors: Vec<NamedCollector<'_, T>>,
    source: CollectionSource,
    scope: &str,
) -> (Vec<T>, CollectionReport) {
    let results = futures::future::join_all(
        collectors
            .into_iter()
            .map(|(name, run)| async move { (name, run.await) }),
    )
    .await;

    let mut collected = Vec::new();
    let mut report = CollectionReport::default();
    for (name, result) in results {
        match result {
            Ok(collection) => collected.push(collection),
            Err(e) => report.record(source, format!("{scope}/{name}"), e),
        }
    }

    (collected, report)
}
