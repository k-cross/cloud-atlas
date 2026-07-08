//! Shared server state: the single live graph plus the patch fan-out channel.
//!
//! The poll loop is the only writer (serialized mutation, per the graph-actor
//! intent in `docs/change_monitoring_design.md` §7); WebSocket connections are
//! readers that also subscribe to the broadcast for incremental patches.

use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::patch::GraphPatch;
use petgraph::graph::Graph;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

/// How many patches a slow WebSocket client may fall behind before the
/// broadcast channel drops the oldest. On lag we resync the client with a fresh
/// snapshot rather than trying to replay, so a modest buffer is fine.
const PATCH_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    /// The authoritative in-memory twin. Cloned cheaply for diffing; replaced
    /// wholesale by the poll loop when a change is detected.
    pub live: Arc<RwLock<Graph<Node, Edge>>>,
    /// Fan-out of incremental patches to every connected client.
    pub patches: broadcast::Sender<GraphPatch>,
}

impl AppState {
    pub fn new(initial: Graph<Node, Edge>) -> Self {
        let (patches, _) = broadcast::channel(PATCH_CHANNEL_CAPACITY);
        Self {
            live: Arc::new(RwLock::new(initial)),
            patches,
        }
    }
}
