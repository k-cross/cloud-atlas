# atlas-server

The Cloud Atlas **live backend**: a long-running server that owns a persistent
in-memory graph, reconciles it against the cloud providers on an interval, and
pushes incremental patches to the frontend over WebSocket. This is Phase 2 of
[`docs/change_monitoring_design.md`](../docs/change_monitoring_design.md); the
CLI (`atlas-cli`) remains the batch/one-shot path.

## Run

```bash
cargo xtask dev --demo                              # the whole stack (server + frontend), supervised
cargo run -p atlas-server -- --demo                 # just this server: credential-free fixtures, port 4681
cargo run -p atlas-server -- --regions us-east-1    # real collection (same provider flags as the CLI)
cargo run -p atlas-server -- --poll-secs 30 --port 8080
```

`--demo` serves the fake multi-cloud fixtures and flips a sentinel node/edge in
and out every other tick, so the whole snapshot → patch → apply path is
exercised with no cloud credentials.

## Design

- **Single-writer graph.** The poll loop (`poll.rs`) is the only mutator, behind
  an `Arc<RwLock<Graph>>`; WebSocket connections are readers. Patches fan out via
  a `tokio::sync::broadcast` channel.
- **Tier-3 reconciliation.** Each tick re-derives the graph
  (`AtlasEngine::collect`, or fixtures in demo), diffs it against the live graph
  (`atlas_lib::atlas::patch::diff`, keyed on stable `node_key`/`edge_key`), and
  broadcasts only non-empty `GraphPatch`es. Event-stream and flow-log feeds
  (Tiers 1–2) plug in on top of this later.

## API

- `GET /snapshot.json` — the full current snapshot (`SNAPSHOT_VERSION` 2).
- `GET /ws` — WebSocket hub. Bidirectional JSON frames:
  - client → server: `{"type":"subscribe"}`, `{"type":"get_snapshot"}`,
    `{"type":"get_neighbors","key":"…"}`
  - server → client: `{"type":"snapshot",…}`, `{"type":"patch",…}`,
    `{"type":"neighbors",…}`, `{"type":"error","message":…}`

CORS is permissive so the `atlas-render/atlas-web` dev server (`bun dev` on
:4680) can connect to `ws://localhost:4681/ws`.
