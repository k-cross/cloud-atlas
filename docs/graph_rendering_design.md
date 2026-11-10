# Graph Rendering Design

Interactive web renderer for large multi-cloud estates, decoupling physics layout (`atlas-layout`, `atlas-layout-wasm`) from WebGL drawing (`atlas-web`).

## Architecture

- **Computation (`atlas-layout` / `atlas-layout-wasm`):** ForceAtlas2 (Barnes-Hut) runs in Rust/Wasm and exposes node coordinates as a flat `Float32Array`. The force kernel is lock-free by construction; native builds parallelize with rayon, while browser Wasm runs single-threaded.
- **Rendering (`atlas-web`):** Each animation frame steps the layout engine, copies coordinates into graphology, and draws via Sigma.js WebGL shaders, plus a separate `canvas.traffic-layer` for animated packet beads.

## UX Principles

- **Context over raw wiring:** Edges convey live activity alongside static topology. Mutable metrics never live on `Node`/`Edge` (which are pure identity types); they travel beside the graph in the snapshot's `observations` list (`change_monitoring_design.md`).
- **Focused views:** Filter views (starting with `GraphController::setServiceView`) hide intermediate plumbing rather than merely dimming it.
- **Search & focus:** `search.ts` ranks nodes by label and kind (every token must match; whole-word before prefix before substring before kind-only, then shorter labels), and long labels show an excerpt around the match. A non-empty query dims every non-match; choosing a result (or clicking a node) focuses it: the camera flies to it, it and its neighbours stay lit, and the traffic canvas mutes dimmed nodes. Search covers only what the current view shows, and patches rebuild matches and focus.
- **Collection health:** The server pushes `/collection.json` as a `collection` frame on connect and on every change; the panel shows held (`holding n/budget scans`), released, malformed and feed-degraded rows (`health.ts`).
- **Hierarchical drill-down:** Group Global → Provider → Region → VPC → Node to avoid visual clutter at scale.

## Remaining Work

| Phase | Deliverable | Status |
|---|---|---|
| **3 (remainder)** | Metadata tooltips, per-node drill-down, and UI inspection of raw `last_seen`/`packets`/`bytes` values (flow styling, freshness halos, packet animation, traffic panel, and service view are built). | In progress |
| **4** | Node and resource search / filter bar. | Done (search, focus, `/` shortcut) |
