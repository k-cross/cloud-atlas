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

    #[test]
    fn a_summary_names_each_failure_by_source_scope_and_kind() {
        let mut report = CollectionReport::default();
        report.note(
            CollectionSource::Aws,
            FailureKind::Unauthorized,
            "us-east-1/credentials",
            "expired token",
        );
        report.note(
            CollectionSource::Cloudflare,
            FailureKind::Malformed,
            "zone globex.io/dns",
            "unknown record type",
        );

        assert_eq!(
            report.summary(),
            "AWS [us-east-1/credentials] unauthorized: expired token; \
             Cloudflare [zone globex.io/dns] malformed: unknown record type"
        );
    }

    #[test]
    fn the_wire_names_are_the_lowercase_ones_the_frontend_matches_on() {
        for (source, shown, wire) in [
            (CollectionSource::Aws, "AWS", "aws"),
            (CollectionSource::Gcp, "GCP", "gcp"),
            (CollectionSource::Azure, "Azure", "azure"),
            (CollectionSource::Cloudflare, "Cloudflare", "cloudflare"),
        ] {
            assert_eq!(source.to_string(), shown);
            assert_eq!(serde_json::to_value(source).unwrap(), wire);
        }
        for (kind, wire) in [
            (FailureKind::Unavailable, "unavailable"),
            (FailureKind::Unauthorized, "unauthorized"),
            (FailureKind::Malformed, "malformed"),
        ] {
            assert_eq!(kind.to_string(), wire);
            assert_eq!(serde_json::to_value(kind).unwrap(), wire);
        }
    }
}
