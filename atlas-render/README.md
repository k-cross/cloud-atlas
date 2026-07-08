# atlas-render

Interactive rendering stack for cloud-atlas (Phases 1–2 of
`docs/graph_rendering_design.md`): a force-directed layout engine that
compiles to WebAssembly, plus a Sigma.js (WebGL) web frontend that renders
it live in the browser.

This is a **separate cargo workspace** on purpose. It never depends on
`atlas-lib` — the cloud SDK dependency tree does not build for
`wasm32-unknown-unknown` and rendering must not be coupled to graph
building. The only contract between the two is the **render snapshot**, a
versioned JSON document (plus a `GraphPatch` delta of the same node/edge
shape, used for live incremental updates):

```json
{
  "version": 2,
  "nodes": [{"id": 0, "key": "AwsEc2Instance#Instance(i-1)", "label": "Instance(i-1)", "kind": "AwsEc2Instance"}],
  "edges": [{"source": 0, "target": 1, "key": "HasIp|...", "source_key": "...", "target_key": "...", "kind": "HasIp"}]
}
```

`key` (added in v2) is a stable identity derived from the typed resource, so a
node or edge can be referenced across full-scan rebuilds — this is what lets
`atlas-server` (below) push add/remove patches instead of re-sending the whole
graph. Producers: `atlas` writes `atlas.json` next to `atlas.dot` on every
update; `atlas-server` serves the live graph at `/snapshot.json` and streams
patches over WebSocket; `cargo run --example demo` in the main workspace
writes `multi_cloud_demo.json` from the credential-free Globex fixtures.

The version constant is pinned on **three** sides — `atlas-lib`'s
`atlas::export::SNAPSHOT_VERSION`, `atlas-layout`'s `SNAPSHOT_VERSION`, and
`atlas-web`'s `graph.ts` — bump all three together when the shape changes,
**and rebuild the wasm** (`bun run wasm`, or just `cargo xtask wasm`): the
compiled layout engine bakes in the version and rejects mismatched snapshots
at runtime.

## Live backend

[`atlas-server`](../atlas-server/README.md) (in the *root* cargo workspace,
not this one — it depends on `atlas-lib`) is the long-running counterpart to
the one-shot `atlas` CLI: it owns a persistent graph, diffs each
reconciliation scan, and pushes `GraphPatch`es to `atlas-web` over WebSocket.
`atlas-web` connects to it by default (`ws://<host>:4681/ws`); pass `?static`
in the URL to force the one-shot `/snapshot.json` fetch instead (what the
rest of this document, and the e2e static tests, exercise). The unified way
to run server + renderer together is `cargo xtask dev --demo` from the repo
root — see the root [`README.md`](../README.md) and `CLAUDE.md`.

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

By default `bun dev` (i.e. `atlas-web`) connects live to `atlas-server` over
WebSocket (`ws://<host>:4681/ws`) — start that first, or use `?static` to fall
back to a one-shot fetch of `atlas.json` from the repo root (or
`multi_cloud_demo.json` if that's absent; pass an explicit path with
`bun serve.ts path/to.json`).

Easiest end-to-end path, live and credential-free, from the repo root:

```sh
cargo xtask dev --demo
```

Or manually, live:

```sh
# 1. Live backend (repo root)
cargo run -p atlas-server -- --demo
# 2. Frontend, connects to it automatically
cd atlas-render/atlas-web && bun dev
```

Or manually, static (no server):

```sh
# 1. Generate the demo snapshot (repo root)
cargo run --example demo
# 2. Verify layout engine natively
cargo run --example layout_demo -- ../multi_cloud_demo.json   # inside atlas-render/
# 3. View in browser with the static fallback
cd atlas-web && bun dev
# then open http://localhost:4680/?static
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
