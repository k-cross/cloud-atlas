# atlas-render

The interactive rendering stack: a ForceAtlas2 layout engine compiled to WebAssembly and a Sigma.js (WebGL) frontend that draws the topology plus the Tier-2 liveness overlay.

This is a **separate cargo workspace** and never depends on `atlas-lib`, whose SDK tree does not build for `wasm32-unknown-unknown`. The only contract is the versioned **render snapshot** (and `GraphPatch`, which has the same node/edge shape):

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

- `key` is a stable identity derived from the typed resource, which is what lets patches add and remove individual elements.
- `observations` is keyed by the same `key` and kept separate because freshness changes far more often than topology; a patch carries `observations` and `expired` without re-announcing resources. Unknown keys are ignored, and the layout engine ignores the list entirely.
- `packets`/`bytes` appear only on `TrafficFlow` edges. A record names several keys, so per-node volume would count the same traffic repeatedly; derive a node's throughput from its incident flow edges. `Serves` observations carry `inferred`/`confirmed` and no volume.

Producers: `atlas` writes `atlas.json`; `atlas-server` serves `/snapshot.json` and streams patches; `cargo run --example demo` (root) writes `multi_cloud_demo.json`.

`SNAPSHOT_VERSION` is pinned in `atlas-lib`'s `atlas::export`, `atlas-layout`, and `atlas-web`'s `graph.ts`. Bump all three together **and rebuild the wasm** (`cargo xtask wasm`), since the compiled engine rejects mismatched versions.

## Crates

- **`atlas-layout`** — pure-Rust ForceAtlas2: degree-weighted repulsion via Barnes-Hut, linear/lin-log attraction, strong gravity, adaptive speed. Deterministic (phyllotaxis-spiral initialization). Positions are one interleaved `[x0, y0, x1, y1, ..]` `f32` buffer.
- **`atlas-layout-wasm`** — `wasm-bindgen` bridge exposing `LayoutEngine`; positions cross as a `Float32Array` (`positionsView()` zero-copy, `positionsCopy()` detached).
- **`atlas-web/`** — SvelteKit + Sigma.js bun app (see its [README](atlas-web/README.md)). Each frame steps the engine, copies positions into graphology, and lets Sigma redraw. Nodes are coloured by provider and sized by degree.

## Build & test

```sh
cargo nextest run --all-targets   # from THIS directory; the root run doesn't reach it
cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown --release
cargo test -p atlas-layout --features parallel

cd atlas-web && bun install && bun run wasm && bun dev   # http://localhost:4680
```

`atlas-web` connects to `atlas-server` at `ws://<host>:4681/ws` by default. `?static` instead fetches `atlas-web/static/snapshot.json`.

End to end, from the repo root:

```sh
cargo xtask dev --demo                        # live, credential-free

# or static, no server:
cargo xtask demo                              # writes multi_cloud_demo.json
cd atlas-render
cargo run --example layout_demo -- ../multi_cloud_demo.json
cp ../multi_cloud_demo.json atlas-web/static/snapshot.json
cd atlas-web && bun dev                       # open /?static
```

## Concurrency

The force kernel writes each node's force into its own slot from shared read-only inputs, so it parallelizes without locks; the `parallel` feature uses rayon natively. Browser wasm runs single-threaded — wasm threads need SharedArrayBuffer (COOP/COEP) and an atomics build, which is deferred.

## Driving it from JS

`atlas-web/src/lib/GraphController.ts` is the real integration; the loop is:

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
