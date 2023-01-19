use petgraph::graph::Graph;
use std::collections::HashMap;

// FIXME: inacurate
#[derive(Debug)]
pub struct GraphTracker {
    graph: ProviderGraph,
    track: HashMap<String, u8>,
}

// FIXME: inacurate
#[derive(Debug)]
pub enum ProviderGraph {
    AWS(Graph<RegionGraph, String>),
    GCP(Graph<RegionGraph, String>),
    Azure(Graph<RegionGraph, String>),
}

// FIXME: inacurate
#[derive(Debug)]
pub enum RegionGraph {
    AWS(Graph<String, String>),
    GCP(Graph<String, String>),
    Azure(Graph<String, String>),
}
