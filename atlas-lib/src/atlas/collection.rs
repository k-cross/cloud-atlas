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

/// One source that could not be read on this scan. `scope` narrows it to the
/// region/project/collector that failed, so a partial scan can be attributed.
#[derive(Debug, Clone, Serialize)]
pub struct CollectionFailure {
    pub source: CollectionSource,
    pub scope: String,
    pub message: String,
}

impl fmt::Display for CollectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.source, self.scope, self.message)
    }
}

/// Everything that went wrong during one scan. An empty report means every
/// configured source was read end to end, and only then is the scan a
/// trustworthy basis for removals.
#[derive(Debug, Default, Clone, Serialize)]
pub struct CollectionReport {
    pub failures: Vec<CollectionFailure>,
}

impl CollectionReport {
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    /// Record a failure carrying an error value from a fallible call. `Debug`
    /// rather than `Display` because that is what preserves an SDK error's
    /// cause chain.
    pub fn record(
        &mut self,
        source: CollectionSource,
        scope: impl Into<String>,
        error: impl fmt::Debug,
    ) {
        self.note(source, scope, format!("{error:?}"));
    }

    /// Record a failure whose cause we are describing ourselves rather than
    /// forwarding from an error value — a row that would not deserialize, a
    /// count of things skipped.
    pub fn note(
        &mut self,
        source: CollectionSource,
        scope: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.failures.push(CollectionFailure {
            source,
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
    pub fn unreadable_sources(&self) -> HashSet<CollectionSource> {
        self.failures.iter().map(|f| f.source).collect()
    }

    pub fn summary(&self) -> String {
        self.failures
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("; ")
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
