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
//!                      {"type":"neighbors", key, nodes, edges}
//!                      {"type":"error", message}

use crate::state::AppState;
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::export::{edge_key, node_key, render_snapshot};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use petgraph::graph::Graph;
use petgraph::visit::EdgeRef;
use serde::Deserialize;
use serde_json::{Value, json};
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

    let mut nodes = vec![render_node_value(graph, center)];
    let mut edges = Vec::new();
    for edge in graph
        .edges(center)
        .chain(graph.edges_directed(center, petgraph::Direction::Incoming))
    {
        let (a, b) = (edge.source(), edge.target());
        let other = if a == center { b } else { a };
        nodes.push(render_node_value(graph, other));
        let sk = node_key(&graph[a]);
        let tk = node_key(&graph[b]);
        edges.push(json!({
            "source": a.index() as u32,
            "target": b.index() as u32,
            "key": edge_key(&sk, &tk, edge.weight()),
            "source_key": sk,
            "target_key": tk,
            "kind": edge.weight().kind(),
        }));
    }

    json!({ "type": "neighbors", "key": key, "nodes": nodes, "edges": edges })
}

fn render_node_value(graph: &Graph<Node, Edge>, i: petgraph::graph::NodeIndex) -> Value {
    json!({
        "id": i.index() as u32,
        "key": node_key(&graph[i]),
        "label": graph[i].to_string(),
        "kind": graph[i].kind(),
    })
}
