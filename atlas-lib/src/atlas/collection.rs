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
use std::fmt;

/// Which provider a collection failure came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Default, Clone)]
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
        scope: impl Into<String>,
        message: impl fmt::Display,
    ) {
        self.failures.push(CollectionFailure {
            source,
            scope: scope.into(),
            message: message.to_string(),
        });
    }

    pub fn absorb(&mut self, failures: Vec<CollectionFailure>) {
        self.failures.extend(failures);
    }

    pub fn summary(&self) -> String {
        self.failures
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// What one provider returned: the collection it managed to build, plus the
/// sub-scopes it could not reach. A provider that fails outright returns `Err`
/// instead; a provider that fetched some of its resources returns `Ok` with a
/// non-empty `failures`, and the caller must treat the result as partial.
#[derive(Debug)]
pub struct ProviderScan {
    pub provider: Provider,
    pub failures: Vec<CollectionFailure>,
}

impl ProviderScan {
    pub fn complete(provider: Provider) -> Self {
        Self {
            provider,
            failures: Vec::new(),
        }
    }
}
