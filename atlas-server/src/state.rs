use crate::health;
use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::flow::FlowIndex;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::patch::{GraphPatch, UnreadableSource};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, watch};

pub const PATCH_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct AppState {
    pub live: Arc<RwLock<GraphBuilder>>,
    pub patches: broadcast::Sender<GraphPatch>,
    pub report: Arc<RwLock<CollectionReport>>,
    pub unreadable: Arc<RwLock<Vec<UnreadableSource>>>,
    pub stream_report: Arc<RwLock<CollectionReport>>,
    pub flows: Arc<RwLock<FlowIndex>>,
    pub flow_report: Arc<RwLock<CollectionReport>>,
    pub health: watch::Sender<Value>,
}

impl AppState {
    pub fn new(initial: GraphBuilder, report: CollectionReport, flows: FlowIndex) -> Self {
        let (patches, _) = broadcast::channel(PATCH_CHANNEL_CAPACITY);
        let clean = CollectionReport::default();
        let (health, _) = watch::channel(health::value(
            &report,
            &[],
            &clean,
            &clean,
            flows.flow_count(),
        ));
        Self {
            live: Arc::new(RwLock::new(initial)),
            patches,
            report: Arc::new(RwLock::new(report)),
            unreadable: Arc::new(RwLock::new(Vec::new())),
            stream_report: Arc::new(RwLock::new(CollectionReport::default())),
            flows: Arc::new(RwLock::new(flows)),
            flow_report: Arc::new(RwLock::new(CollectionReport::default())),
            health,
        }
    }
}
