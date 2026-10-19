# atlas-render

Interactive rendering workspace: a Rust/WebAssembly ForceAtlas2 layout engine (`atlas-layout`, `atlas-layout-wasm`) and a SvelteKit + Sigma.js WebGL frontend (`atlas-web`).

This is a **separate Cargo workspace** that never depends on `atlas-lib` (whose cloud SDK dependencies do not target `wasm32-unknown-unknown`). The sole contract is the versioned **render snapshot** and matching `GraphPatch`:

```json
{
  "version": 3,
  "nodes": [{"id": 0, "key": "AwsEc2Instance#Instance(i-1)", "label": "Instance(i-1)", "kind": "AwsEc2Instance"}],
  "edges": [{"source": 0, "target": 1, "key": "HasIp|...", "source_key": "...", "target_key": "...", "kind": "HasIp"}],
  "observations": [
    {"key": "TrafficFlow|...", "last_seen": 1788436800000, "packets": 24, "bytes": 4800, "status": "accepted"},
    {"key": "GenericIpAddress#...", "last_seen": 1788436800000, "status": "accepted"},
    {"key": "Serves|...", "last_seen": 0, "status": "inferred"}
  ]
}
```

- **`key`:** Stable identity derived from the typed resource, enabling incremental add/remove patches.
- **`observations`:** Keyed by `key` (ignored by the layout engine). Patches send updated `observations` and `expired` keys without re-announcing topology. `packets`/`bytes` appear only on `TrafficFlow` edges to avoid multi-counting across incident nodes; `Serves` carries `inferred`/`confirmed` status and `last_seen`.
- **`SNAPSHOT_VERSION` (3):** Pinned in `atlas-lib/src/atlas/export.rs`, `atlas-layout/src/graph.rs`, and `atlas-web/src/lib/graph.ts`. Bump all three together and run `cargo xtask wasm`.

## Crates

- **`atlas-layout`:** Deterministic ForceAtlas2 (phyllotaxis initialization, degree-weighted Barnes-Hut repulsion, linear/lin-log attraction, strong gravity, adaptive speed) over an interleaved `[x0, y0, x1, y1, …]` `f32` buffer. Lock-free kernel; optional `parallel` feature uses rayon natively.
- **`atlas-layout-wasm`:** `wasm-bindgen` wrapper exposing `LayoutEngine` (`positionsView()` zero-copy `Float32Array`, `positionsCopy()` detached).
- **`atlas-web/`:** SvelteKit + Sigma.js Bun app (see [`atlas-web/README.md`](atlas-web/README.md)).

## Build & Test

```sh
# From repo root (preferred)
cargo xtask dev --demo
cargo xtask test [--e2e]

# Inside atlas-render/
cargo nextest run --all-targets
cargo test -p atlas-layout --features parallel
cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown --release
cd atlas-web && bun install && bun run wasm && bun dev   # http://localhost:4680 (or /?static)
```

## Minimal JS Loop

See `atlas-web/src/lib/GraphController.ts` for the full integration:

```js
import init, { LayoutEngine } from "../../static/pkg/atlas_layout_wasm.js";

await init();
const engine = new LayoutEngine(await (await fetch("/snapshot.json")).text());
function frame() {
  engine.step(5);
  draw(engine.positionsView());
  if (engine.speed() > 0.01) requestAnimationFrame(frame);
}
frame();
```
