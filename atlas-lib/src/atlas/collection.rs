use crate::cloud::definition::Provider;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionSource {
    Aws,
    Gcp,
    Azure,
    Cloudflare,
}

impl fmt::Display for CollectionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            CollectionSource::Aws => "AWS",
            CollectionSource::Gcp => "GCP",
            CollectionSource::Azure => "Azure",
            CollectionSource::Cloudflare => "Cloudflare",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureKind {
    Unavailable,
    Unauthorized,
    Malformed,
}

impl FailureKind {
    pub fn is_unreadable(self) -> bool {
        !matches!(self, FailureKind::Malformed)
    }
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            FailureKind::Unavailable => "unavailable",
            FailureKind::Unauthorized => "unauthorized",
            FailureKind::Malformed => "malformed",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionFailure {
    pub source: CollectionSource,
    pub kind: FailureKind,
    pub scope: String,
    pub message: String,
}

impl fmt::Display for CollectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] {}: {}",
            self.source, self.scope, self.kind, self.message
        )
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct CollectionReport {
    pub failures: Vec<CollectionFailure>,
}

impl CollectionReport {
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn record(
        &mut self,
        source: CollectionSource,
        kind: FailureKind,
        scope: impl Into<String>,
        error: impl fmt::Debug,
    ) {
        self.note(source, kind, scope, format!("{error:?}"));
    }

    pub fn note(
        &mut self,
        source: CollectionSource,
        kind: FailureKind,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.failures.push(CollectionFailure {
            source,
            kind,
            scope: scope.into(),
            message: message.into(),
        });
    }

    pub fn merge(&mut self, other: CollectionReport) {
        self.failures.extend(other.failures);
    }

    pub fn unreadable_sources(&self) -> HashSet<CollectionSource> {
        self.failures
            .iter()
            .filter(|f| f.kind.is_unreadable())
            .map(|f| f.source)
            .collect()
    }

    pub fn unreadable_kind(&self, source: CollectionSource) -> Option<FailureKind> {
        let mut kinds = self
            .failures
            .iter()
            .filter(|f| f.source == source && f.kind.is_unreadable())
            .map(|f| f.kind)
            .peekable();

        kinds.peek()?;
        if kinds.all(|kind| kind == FailureKind::Unauthorized) {
            Some(FailureKind::Unauthorized)
        } else {
            Some(FailureKind::Unavailable)
        }
    }

    pub fn summary(&self) -> String {
        self.failures
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(failures: &[(CollectionSource, FailureKind)]) -> CollectionReport {
        let mut report = CollectionReport::default();
        for (source, kind) in failures {
            report.note(*source, *kind, "scope", "message");
        }
        report
    }

    #[test]
    fn a_malformed_record_does_not_make_its_source_unreadable() {
        let report = report(&[(CollectionSource::Azure, FailureKind::Malformed)]);

        assert!(!report.is_complete(), "something was still lost");
        assert!(report.unreadable_sources().is_empty());
        assert_eq!(report.unreadable_kind(CollectionSource::Azure), None);
    }

    #[test]
    fn a_source_with_only_refusals_is_diagnosed_unauthorized() {
        let report = report(&[
            (CollectionSource::Aws, FailureKind::Unauthorized),
            (CollectionSource::Aws, FailureKind::Unauthorized),
        ]);

        assert_eq!(
            report.unreadable_kind(CollectionSource::Aws),
            Some(FailureKind::Unauthorized)
        );
    }

    #[test]
    fn one_reachable_failure_downgrades_the_diagnosis() {
        let report = report(&[
            (CollectionSource::Aws, FailureKind::Unauthorized),
            (CollectionSource::Aws, FailureKind::Unavailable),
        ]);

        assert_eq!(
            report.unreadable_kind(CollectionSource::Aws),
            Some(FailureKind::Unavailable)
        );
    }

    #[test]
    fn a_malformed_record_does_not_downgrade_a_refusal() {
        let report = report(&[
            (CollectionSource::Aws, FailureKind::Unauthorized),
            (CollectionSource::Aws, FailureKind::Malformed),
        ]);

        assert_eq!(
            report.unreadable_kind(CollectionSource::Aws),
            Some(FailureKind::Unauthorized)
        );
    }

    #[test]
    fn failures_stay_attributed_to_their_own_source() {
        let report = report(&[
            (CollectionSource::Aws, FailureKind::Unauthorized),
            (CollectionSource::Gcp, FailureKind::Unavailable),
        ]);

        assert_eq!(
            report.unreadable_kind(CollectionSource::Aws),
            Some(FailureKind::Unauthorized)
        );
        assert_eq!(
            report.unreadable_kind(CollectionSource::Gcp),
            Some(FailureKind::Unavailable)
        );
        assert_eq!(report.unreadable_kind(CollectionSource::Cloudflare), None);
    }
}

#[derive(Debug)]
pub struct ProviderScan {
    pub provider: Provider,
    pub report: CollectionReport,
}
