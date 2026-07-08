//! HTTP surface: the one-shot snapshot (also handy for `curl`/static viewers)
//! and the WebSocket upgrade. CORS is permissive so the bun dev server on a
//! different port can talk to us during development.

use crate::state::AppState;
use crate::ws;
use atlas_lib::atlas::export::render_snapshot;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use tower_http::cors::CorsLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/snapshot.json", get(snapshot))
        .route("/ws", get(ws::handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    let graph = state.live.read().await;
    Json(render_snapshot(&graph))
}
