# Graph Rendering Design

Replace static `.dot` output with an interactive web view that stays responsive on large multi-cloud estates.

## Approach

Layout and drawing are decoupled so physics never blocks the UI.

- **Computation — Rust/WebAssembly.** ForceAtlas2 (Barnes-Hut) runs in a wasm module and exposes positions as a flat `Float32Array`. The kernel is lock-free by construction, so native builds parallelize with rayon; browser threading is deferred.
- **Rendering — Sigma.js (WebGL).** Positions are copied into graphology each frame and drawn by WebGL shaders, bypassing the DOM.

## UX principles

- **Context over raw connections.** Edges should say what is happening, not just what is wired. Metrics never live on the edge itself — `Node`/`Edge` are identity types — so they travel beside the graph as the snapshot's `observations` (see `change_monitoring_design.md`, Tier 2).
- **Hierarchical drill-down.** Group Global → Provider → Region → VPC → Node to avoid a hairball.
- **Filterable.** Views that hide what is not relevant (the service view is the first).

## Phases

| Phase | Deliverable | Status |
|---|---|---|
| 1 | wasm layout engine | Done — `atlas-layout`, `atlas-layout-wasm` |
| 2 | Sigma.js frontend | Done — `atlas-web` |
| 3 | Contextual data | Mostly done — flow edges by verdict and volume, freshness halos, packet animation, traffic panel, `Serves` status styling and service view. Remaining: metadata tooltips, per-node drill-down, a way to read raw `last_seen`/`packets`/`bytes`. |
| 4 | Search | Not started |
