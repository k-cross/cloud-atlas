//! Outcome reporting for a collection scan.
//!
//! A scan that fails to reach a provider is *not* the same as a scan that found
//! nothing there, but both used to arrive at the projector as "no collection".
//! Under the live server that ambiguity is destructive: `patch::diff` reads the
//! absence as deletion and broadcasts a removal for every node the failed
//! source owns, which the next healthy tick puts straight back. These types let
//! a scan say which sources it actually reached, so the poll loop can decline
//! to garbage-collect on incomplete data.

use crate::cloud::definition::Provider;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;

/// Which provider a collection failure came from.
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

/// Why a scope could not be read, and therefore how long the graph should keep
/// waiting for it.
///
/// Retention is a bet on recovery: holding a resource is the right answer to a
/// transient failure and the wrong answer to a permanent one. A stringified
/// error cannot tell those apart, so every failure is classified once — where
/// the error is still typed — and [`crate::atlas::patch::Retention`] spends a
/// different budget on each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureKind {
    /// Could not be reached, but may come back on its own: throttling, a
    /// timeout, a 5xx, a dropped connection. Worth waiting out.
    Unavailable,
    /// Reached and refused: missing, expired, or insufficient credentials. A
    /// timer does not fix this, so the graph stops claiming those resources
    /// sooner than it would for an outage.
    Unauthorized,
    /// Read successfully, but part of the response could not be understood — a
    /// drifted row, an unmappable resource type.
    ///
    /// This is *not* an unreadable source, and that distinction is the whole
    /// point of the kind: everything else in the response is authoritative, so
    /// a malformed record must never trigger carry-forward. Azure returns the
    /// entire tenant in one response, so before this existed a single row that
    /// would not deserialize froze deletions across every Azure resource for
    /// the full retention budget.
    Malformed,
}

impl FailureKind {
    /// Whether a failure of this kind means the source could not be read, and
    /// so cannot be trusted about what is gone.
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

/// One thing that went wrong on this scan. `scope` narrows it to the
/// region/project/collector that failed, so a partial scan can be attributed,
/// and `kind` says whether it stops the source being authoritative.
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

/// Everything that went wrong during one scan. An empty report means every
/// configured source was read end to end and nothing in it was dropped.
#[derive(Debug, Default, Clone, Serialize)]
pub struct CollectionReport {
    pub failures: Vec<CollectionFailure>,
}

impl CollectionReport {
    /// Whether the scan lost nothing at all. Note this is *stricter* than being
    /// a trustworthy basis for removals: a scan carrying only
    /// [`FailureKind::Malformed`] failures is incomplete but still
    /// authoritative, because every source was read. Use
    /// [`Self::unreadable_sources`] for the removal question.
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    /// Record a failure carrying an error value from a fallible call. `Debug`
    /// rather than `Display` because that is what preserves an SDK error's
    /// cause chain.
    ///
    /// `kind` is required rather than inferred: by the time an error reaches a
    /// report it is usually a `Box<dyn Error>`, and guessing a permanent
    /// failure from a stringified one is exactly the fragility this replaces.
    /// Classify at the call site, where the error is still typed.
    pub fn record(
        &mut self,
        source: CollectionSource,
        kind: FailureKind,
        scope: impl Into<String>,
        error: impl fmt::Debug,
    ) {
        self.note(source, kind, scope, format!("{error:?}"));
    }

    /// Record a failure whose cause we are describing ourselves rather than
    /// forwarding from an error value — a row that would not deserialize, a
    /// count of things skipped.
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

    /// The providers this scan cannot speak for. Removals stay authoritative
    /// for every source *not* in this set, so one throttled AWS collector must
    /// not stop the differ from deleting a genuinely gone Cloudflare zone.
    ///
    /// [`FailureKind::Malformed`] failures are deliberately excluded: the
    /// response *was* read, so the source is still authoritative about what it
    /// contains. Only what could not be reached suspends removals.
    pub fn unreadable_sources(&self) -> HashSet<CollectionSource> {
        self.failures
            .iter()
            .filter(|f| f.kind.is_unreadable())
            .map(|f| f.source)
            .collect()
    }

    /// Which kind of failure should decide how long `source` is held, or `None`
    /// if it was readable.
    ///
    /// A source counts as [`FailureKind::Unauthorized`] only when *every*
    /// failure that made it unreadable was a permissions problem. Mixed
    /// evidence — one region forbidden, another throttled — may still recover
    /// on its own, so it keeps the longer budget. Releasing early is only safe
    /// when the diagnosis is unambiguous.
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

    /// Mixed evidence keeps the more forgiving diagnosis: something here may
    /// still recover on its own.
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

    /// A malformed record sits alongside a refusal without softening it.
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

/// What one provider returned: the collection it managed to build, plus every
/// scope it could not reach. `build_*` is infallible by construction — a
/// provider that fails outright still returns a scan, with the empty collection
/// explained by a non-empty `report`. There is no second channel for failure,
/// so no caller can mistake a dead provider for an empty one.
#[derive(Debug)]
pub struct ProviderScan {
    pub provider: Provider,
    pub report: CollectionReport,
}
