# atlas-render

Interactive rendering stack for cloud-atlas (Phase 1 of
`docs/graph_rendering_design.md`): a force-directed layout engine that
compiles to WebAssembly.

This is a **separate cargo workspace** on purpose. It never depends on
`atlas-lib` — the cloud SDK dependency tree does not build for
`wasm32-unknown-unknown` and rendering must not be coupled to graph
building. The only contract between the two is the **render snapshot**, a
versioned JSON document:

```json
{
  "version": 1,
  "nodes": [{"id": 0, "label": "Instance(i-1)", "kind": "AwsEc2Instance"}],
  "edges": [{"source": 0, "target": 1, "kind": "HasIp"}]
}
```

Producers: `atlas` writes `atlas.json` next to `atlas.dot` on every update
(including each daemon poll); `cargo run --example demo` in the main
workspace writes `multi_cloud_demo.json` from the credential-free Globex
fixtures. The version constant is pinned on both sides (`atlas-lib`'s
`atlas::export::SNAPSHOT_VERSION` and `atlas-layout`'s `SNAPSHOT_VERSION`) —
bump both together when the shape changes.

## Crates

- **`atlas-layout`** — pure-Rust ForceAtlas2: degree-weighted repulsion with
  a Barnes-Hut quadtree, linear/lin-log attraction, (strong) gravity, and the
  paper's adaptive speed controller. Deterministic: phyllotaxis-spiral
  initialization, no randomness. Positions live in one interleaved
  `[x0, y0, x1, y1, ..]` `f32` buffer.
- **`atlas-layout-wasm`** — thin `wasm-bindgen` bridge exposing
  `LayoutEngine` to JavaScript. Positions cross the boundary as a
  `Float32Array` (zero-copy `positionsView()` or detached
  `positionsCopy()`), ready for Sigma.js in Phase 2.

## Build & test

```sh
cargo test                                            # native unit tests
cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown --release
```

For a browser-ready package (JS glue + `.d.ts`):

```sh
cargo install wasm-pack
wasm-pack build atlas-layout-wasm --target web --release
```

## Concurrency

The force kernel writes each node's force into its own slot of a separate
force buffer from shared read-only inputs, so it parallelizes without locks.
The `parallel` feature (rayon) enables this natively today:

```sh
cargo test -p atlas-layout --features parallel
```

Browser wasm runs the same code single-threaded for now: wasm threads
require SharedArrayBuffer (COOP/COEP headers) and an atomics-enabled build
(e.g. `wasm-bindgen-rayon`). That is deliberately out of scope for Phase 1;
the kernel shape already fits it.

## Driving it from JS (Phase 2 preview)

```js
import init, { LayoutEngine } from "./pkg/atlas_layout_wasm.js";

await init();
const engine = new LayoutEngine(await (await fetch("atlas.json")).text());
function frame() {
  engine.step(5);                              // physics budget per frame
  draw(engine.positionsView());                // zero-copy Float32Array
  if (engine.speed() > 0.01) requestAnimationFrame(frame);
}
frame();
```
