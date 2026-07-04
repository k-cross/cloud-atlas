# atlas-render

Interactive rendering stack for cloud-atlas (Phases 1–2 of
`docs/graph_rendering_design.md`): a force-directed layout engine that
compiles to WebAssembly, plus a Sigma.js (WebGL) web frontend that renders
it live in the browser.

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
  `positionsCopy()`), consumed by `atlas-web`.
- **`atlas-web/`** — Sigma.js (WebGL) frontend, a bun app rather than a
  cargo crate. Each animation frame it steps the wasm engine, copies the
  position buffer into graphology node attributes, and lets Sigma redraw.
  Nodes are colored by provider (derived from the snapshot `kind` prefix)
  and sized by degree; a panel shows live layout status, per-provider
  counts, and a reheat button.

## Build & test

```sh
# Rust unit tests (atlas-layout, atlas-layout-wasm)
cargo test

# Check wasm compilation without the JS glue
cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown --release
```

The web frontend uses [bun](https://bun.sh) (`wasm-pack` is a bun dev
dependency — no separate global install needed):

```sh
cd atlas-web
bun install
bun run wasm       # wasm-pack build → atlas-web/pkg/ (JS glue + .wasm)
bun dev            # http://localhost:4680
```

`bun dev` serves `atlas.json` from the repo root if present, else
`multi_cloud_demo.json` (generate it with `cargo run --example demo` in the
root workspace); pass an explicit path with `bun serve.ts path/to.json`.

End-to-end without credentials:

```sh
# 1. Generate the demo snapshot (repo root)
cargo run --example demo
# 2. Verify layout engine natively
cargo run --example layout_demo -- ../multi_cloud_demo.json   # inside atlas-render/
# 3. View in browser
cd atlas-web && bun dev
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
(e.g. `wasm-bindgen-rayon`). That is deliberately deferred; the kernel
shape already fits it.

## Driving it from JS

`atlas-web/src/main.ts` is the real integration; the shape of the loop:

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
