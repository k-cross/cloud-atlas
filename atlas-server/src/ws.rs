//! WebSocket hub. The connection is bidirectional: the server streams patches
//! as they happen, and the client can pull specific data on demand (the full
//! snapshot, or the neighborhood of one node) rather than only listening.
//!
//! Protocol (JSON text frames):
//!   client -> server: {"type":"subscribe"}            resend snapshot, then stream patches
//!                      {"type":"get_snapshot"}         full current snapshot
//!                      {"type":"get_neighbors","key"}  subgraph around one node
//!   server -> client: {"type":"snapshot", version, nodes, edges}
//!                      {"type":"patch", ...GraphPatch}
//!                      {"type":"neighbors", version, key, nodes, edges}
//!                      {"type":"error", message}

use crate::state::AppState;
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::export::{
    RenderEdge, RenderNode, SNAPSHOT_VERSION, node_key, render_snapshot,
};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast::error::RecvError;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Subscribe,
    GetSnapshot,
    GetNeighbors { key: String },
}

pub async fn handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| connection(socket, state))
}

async fn connection(socket: WebSocket, state: AppState) {
    // Subscribe *before* the first snapshot so no patch is missed in the gap
    // (delivery is at-least-once; the frontend applies patches tolerantly).
    let mut patches = state.patches.subscribe();
    let (mut sink, mut stream) = socket.split();

    // Push an initial snapshot immediately so a client that just connects and
    // listens still renders without having to ask.
    if send_snapshot(&mut sink, &state).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if handle_client_msg(&mut sink, &state, &text).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ignore ping/pong/binary
                Some(Err(_)) => break,
            },
            patch = patches.recv() => match patch {
                Ok(patch) => {
                    let mut v = serde_json::to_value(&patch).unwrap_or_else(|_| json!({}));
                    v["type"] = json!("patch");
                    if send_value(&mut sink, &v).await.is_err() {
                        break;
                    }
                }
                // Fell behind the buffer — resync with a full snapshot.
                Err(RecvError::Lagged(_)) => {
                    if send_snapshot(&mut sink, &state).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break,
            },
        }
    }
}

async fn handle_client_msg(
    sink: &mut (impl SinkExt<Message> + Unpin),
    state: &AppState,
    text: &str,
) -> Result<(), ()> {
    match serde_json::from_str::<ClientMsg>(text) {
        Ok(ClientMsg::Subscribe) | Ok(ClientMsg::GetSnapshot) => send_snapshot(sink, state).await,
        Ok(ClientMsg::GetNeighbors { key }) => {
            let graph = state.live.read().await;
            let value = neighbors_value(&graph, &key);
            send_value(sink, &value).await
        }
        Err(e) => {
            let value = json!({ "type": "error", "message": format!("bad message: {e}") });
            send_value(sink, &value).await
        }
    }
}

async fn send_snapshot(
    sink: &mut (impl SinkExt<Message> + Unpin),
    state: &AppState,
) -> Result<(), ()> {
    let value = {
        let graph = state.live.read().await;
        let mut v = serde_json::to_value(render_snapshot(&graph)).unwrap_or_else(|_| json!({}));
        v["type"] = json!("snapshot");
        v
    };
    send_value(sink, &value).await
}

async fn send_value(sink: &mut (impl SinkExt<Message> + Unpin), value: &Value) -> Result<(), ()> {
    sink.send(Message::Text(value.to_string()))
        .await
        .map_err(|_| ())
}

/// The node with `key` plus its immediate neighbors and the edges between them,
/// in the same node/edge shape as the snapshot so the frontend can reuse its
/// render path.
fn neighbors_value(graph: &Graph<Node, Edge>, key: &str) -> Value {
    let center = graph.node_indices().find(|&i| node_key(&graph[i]) == key);

    let Some(center) = center else {
        return json!({ "type": "error", "message": format!("no node with key {key}") });
    };

    let mut local_ids: HashMap<NodeIndex, u32> = HashMap::new();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen_edges = HashSet::new();

    local_ids.insert(center, 0);
    nodes.push(RenderNode::new(&graph[center], 0));

    for edge in graph
        .edges(center)
        .chain(graph.edges_directed(center, petgraph::Direction::Incoming))
    {
        if !seen_edges.insert(edge.id()) {
            continue;
        }

        let (a, b) = (edge.source(), edge.target());
        for endpoint in [a, b] {
            if let Entry::Vacant(slot) = local_ids.entry(endpoint) {
                let local_id = nodes.len() as u32;
                slot.insert(local_id);
                nodes.push(RenderNode::new(&graph[endpoint], local_id));
            }
        }

        edges.push(RenderEdge::new(
            &graph[a],
            &graph[b],
            edge.weight(),
            local_ids[&a],
            local_ids[&b],
        ));
    }

    json!({
        "type": "neighbors",
        "version": SNAPSHOT_VERSION,
        "key": key,
        "nodes": nodes,
        "edges": edges,
    })
}

#[cfg(test)]
mod tests {
    use super::neighbors_value;
    use atlas_lib::atlas::definition::{Edge, Node};
    use atlas_lib::atlas::export::{SNAPSHOT_VERSION, node_key};
    use petgraph::graph::Graph;

    #[test]
    fn neighbors_indices_address_the_payloads_own_node_array() {
        let mut graph: Graph<Node, Edge> = Graph::new();
        let center = graph.add_node(Node::GenericHostname("center.example".into()));
        let filler: Vec<_> = (0..8)
            .map(|i| graph.add_node(Node::GenericIpAddress(format!("192.0.2.{i}").into())))
            .collect();
        let peer = graph.add_node(Node::GenericIpAddress("10.0.0.1".into()));

        graph.add_edge(center, peer, Edge::ResolvesTo);
        graph.add_edge(center, peer, Edge::RoutesTo);
        graph.add_edge(peer, center, Edge::RoutesTo);
        graph.add_edge(center, center, Edge::RoutesTo);
        graph.add_edge(filler[0], filler[1], Edge::RoutesTo);

        let value = neighbors_value(&graph, &node_key(&graph[center]));
        let nodes = value["nodes"].as_array().expect("nodes array");
        let edges = value["edges"].as_array().expect("edges array");

        assert_eq!(
            nodes.len(),
            2,
            "a neighbor reachable by several edges must appear once"
        );
        assert_eq!(edges.len(), 4, "the self-loop must not be emitted twice");

        for (position, node) in nodes.iter().enumerate() {
            assert_eq!(node["id"].as_u64(), Some(position as u64));
        }

        for edge in edges {
            for endpoint in ["source", "target"] {
                let index = edge[endpoint].as_u64().expect("endpoint index");
                assert!(
                    (index as usize) < nodes.len(),
                    "{endpoint} {index} is outside the payload's {} nodes",
                    nodes.len()
                );
                assert_eq!(
                    nodes[index as usize]["key"].as_str(),
                    edge[&format!("{endpoint}_key")].as_str(),
                    "positional index and stable key must name the same node"
                );
            }
        }

        assert_eq!(value["version"].as_u64(), Some(SNAPSHOT_VERSION as u64));
    }

    /// The frontend renders both payloads through one path, so the neighbors
    /// subgraph must carry exactly the snapshot's fields. Both build through
    /// `RenderNode`/`RenderEdge` to make that true by construction; this fails
    /// if anyone hand-rolls the shape again and a `SNAPSHOT_VERSION` bump then
    /// reaches only one of them.
    #[test]
    fn a_neighbors_payload_has_the_same_shape_as_a_snapshot() {
        use atlas_lib::atlas::export::render_snapshot;

        let mut graph: Graph<Node, Edge> = Graph::new();
        let a = graph.add_node(Node::GenericHostname("center.example".into()));
        let b = graph.add_node(Node::GenericIpAddress("10.0.0.1".into()));
        graph.add_edge(a, b, Edge::ResolvesTo);

        let neighbors = neighbors_value(&graph, &node_key(&graph[a]));
        let snapshot = serde_json::to_value(render_snapshot(&graph)).expect("serializes");

        for collection in ["nodes", "edges"] {
            let from_neighbors = neighbors[collection][0].as_object().expect(collection);
            let from_snapshot = snapshot[collection][0].as_object().expect(collection);
            assert_eq!(
                from_neighbors.keys().collect::<Vec<_>>(),
                from_snapshot.keys().collect::<Vec<_>>(),
                "{collection} drifted between the two payloads"
            );
        }
    }

    #[test]
    fn neighbors_reports_an_error_for_an_unknown_key() {
        let graph: Graph<Node, Edge> = Graph::new();
        let value = neighbors_value(&graph, "nope");
        assert_eq!(value["type"].as_str(), Some("error"));
    }
}
