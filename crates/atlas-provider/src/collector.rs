use atlas_graph::collection::{CollectionReport, CollectionSource, FailureKind};
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::task::Poll;

    const NAMES: &[&str] = &[
        "ec2", "lambda", "rds", "sqs", "sns", "eks", "dns", "zones", "kv", "r2", "vnets", "nics",
    ];

    fn source() -> impl Strategy<Value = CollectionSource> {
        prop_oneof![
            Just(CollectionSource::Aws),
            Just(CollectionSource::Gcp),
            Just(CollectionSource::Azure),
            Just(CollectionSource::Cloudflare),
        ]
    }

    async fn yield_times(times: u8) {
        for _ in 0..times {
            let mut yielded = false;
            futures::future::poll_fn(|cx| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
        }
    }

    proptest! {
        #[test]
        fn every_collector_lands_once_as_a_collection_or_an_attributed_failure(
            outcomes in prop::collection::vec((any::<bool>(), 0u8..4), 0..=NAMES.len()),
            source in source(),
        ) {
            let collectors: Vec<NamedCollector<'_, usize>> = outcomes
                .iter()
                .enumerate()
                .map(|(at, &(succeeds, yields))| {
                    let name = NAMES[at];
                    let run = async move {
                        yield_times(yields).await;
                        if succeeds {
                            Ok(at)
                        } else {
                            Err(format!("{name} refused").into())
                        }
                    };
                    (name, Box::pin(run) as _)
                })
                .collect();

            let (collected, report) =
                futures::executor::block_on(run_all(collectors, source, "us-east-1"));

            let succeeded: Vec<usize> = outcomes
                .iter()
                .enumerate()
                .filter(|(_, (succeeds, _))| *succeeds)
                .map(|(at, _)| at)
                .collect();
            prop_assert_eq!(collected, succeeded);

            let failed: Vec<&str> = outcomes
                .iter()
                .enumerate()
                .filter(|(_, (succeeds, _))| !*succeeds)
                .map(|(at, _)| NAMES[at])
                .collect();
            prop_assert_eq!(report.failures.len(), failed.len());
            for (failure, name) in report.failures.iter().zip(&failed) {
                let refusal = format!("{name} refused");
                prop_assert_eq!(&failure.scope, &format!("us-east-1/{name}"));
                prop_assert!(failure.message.contains(&refusal));
                prop_assert_eq!(failure.source, source);
                prop_assert_eq!(failure.kind, FailureKind::Unavailable);
            }
            prop_assert_eq!(report.is_complete(), failed.is_empty());
        }
    }
}
