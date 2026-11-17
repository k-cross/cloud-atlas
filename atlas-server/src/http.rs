use crate::state::AppState;
use crate::ws;
use atlas_graph::derive;
use atlas_graph::export::render_snapshot_with;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use tower_http::cors::CorsLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/snapshot.json", get(snapshot))
        .route("/collection.json", get(collection))
        .route("/ws", get(ws::handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    let live = state.live.read().await;
    let flows = state.flows.read().await;
    Json(render_snapshot_with(
        &live.graph,
        derive::observations(&live.graph, &flows),
    ))
}

async fn collection(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.health.borrow().clone())
}
