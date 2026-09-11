use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind};
use std::future::Future;
use std::pin::Pin;

pub type NamedCollector<'a, T> = (
    &'static str,
    Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error>>> + 'a>>,
);

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

            Err(e) => report.record(
                source,
                FailureKind::Unavailable,
                format!("{scope}/{name}"),
                e,
            ),
        }
    }

    (collected, report)
}
