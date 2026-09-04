//! Shared server state: the single live graph plus the patch fan-out channel.
//!
//! The poll loop is the only writer (serialized mutation, per the graph-actor
//! intent in `docs/change_monitoring_design.md` §7); WebSocket connections are
//! readers that also subscribe to the broadcast for incremental patches.

use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::patch::GraphPatch;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

/// How many patches a slow WebSocket client may fall behind before the
/// broadcast channel drops the oldest. On lag we resync the client with a fresh
/// snapshot rather than trying to replay, so a modest buffer is fine.
pub const PATCH_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    /// The authoritative in-memory twin, held as a [`GraphBuilder`] rather than
    /// a bare `Graph` for its node index: the Tier-1 event path mutates single
    /// resources by identity, and without the index every event would mean a
    /// linear scan of the graph to find the node it names. The Tier-3 loop
    /// already produces a builder, so keeping it costs nothing.
    pub live: Arc<RwLock<GraphBuilder>>,
    /// Fan-out of incremental patches to every connected client.
    pub patches: broadcast::Sender<GraphPatch>,
    /// What the most recent scan could not read. Without this a client cannot
    /// tell a small estate from a graph collected during an outage, since both
    /// look like a snapshot that is simply missing those resources.
    pub report: Arc<RwLock<CollectionReport>>,
    /// What the Tier-1 event feed could not read, kept *separate* from the scan
    /// report on purpose. A dead event stream means the graph is slow, not
    /// wrong: Tier 3 still reads the same provider end to end and is still
    /// authoritative about what is gone. Folding stream failures into the scan
    /// report would make an unreachable queue suspend removals across all of
    /// AWS, which is precisely backwards.
    pub stream_report: Arc<RwLock<CollectionReport>>,
}

impl AppState {
    pub fn new(initial: GraphBuilder, report: CollectionReport) -> Self {
        let (patches, _) = broadcast::channel(PATCH_CHANNEL_CAPACITY);
        Self {
            live: Arc::new(RwLock::new(initial)),
            patches,
            report: Arc::new(RwLock::new(report)),
            stream_report: Arc::new(RwLock::new(CollectionReport::default())),
        }
    }
}
