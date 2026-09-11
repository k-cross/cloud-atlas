# atlas-render

Interactive rendering stack for cloud-atlas (Phases 1–3 of
`docs/graph_rendering_design.md`): a force-directed layout engine that
compiles to WebAssembly, plus a Sigma.js (WebGL) web frontend that renders
it live in the browser — topology from the graph, and the Tier-2 liveness
overlay drawn on top of it as animated traffic.

This is a **separate cargo workspace** on purpose. It never depends on
`atlas-lib` — the cloud SDK dependency tree does not build for
`wasm32-unknown-unknown` and rendering must not be coupled to graph
building. The only contract between the two is the **render snapshot**, a
versioned JSON document (plus a `GraphPatch` delta of the same node/edge
shape, used for live incremental updates):

```json
{
  "version": 3,
  "nodes": [{"id": 0, "key": "AwsEc2Instance#Instance(i-1)", "label": "Instance(i-1)", "kind": "AwsEc2Instance"}],
  "edges": [{"source": 0, "target": 1, "key": "HasIp|...", "source_key": "...", "target_key": "...", "kind": "HasIp"}],
  "observations": [
    {"key": "TrafficFlow|...", "last_seen": 1788436800000, "packets": 24, "bytes": 4800, "status": "accepted"},
    {"key": "GenericIpAddress#...", "last_seen": 1788436800000, "status": "accepted"}
  ]
}
```

`key` (added in v2) is a stable identity derived from the typed resource, so a
node or edge can be referenced across full-scan rebuilds — this is what lets
`atlas-server` (below) push add/remove patches instead of re-sending the whole
graph.

`observations` (added in v3) is the Tier-2 liveness overlay: how recently
traffic was seen on a node or edge, keyed by that same stable `key`. It is a
separate list rather than fields on the node because freshness changes on a
completely different cadence from topology — a patch carries `observations`
(refreshed) and `expired` (lapsed keys) without re-announcing a single
resource. An observation naming a key the graph does not hold is ignored; the
layout engine ignores the whole list, since it positions by topology alone.

`packets`/`bytes` appear on flow **edges** only. One record names several keys —
both endpoints plus the instance and interface it came from — so stamping volume
on each would report the same traffic several times over, and on a node the
figure would be an undirected sum across every flow that touched it anyway. A
node carries `last_seen` and `status`, which compose as a maximum and a flag;
derive its throughput from its incident `TrafficFlow` edges. Producers: `atlas` writes `atlas.json` next to `atlas.dot` on every
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
- **`atlas-web/`** — SvelteKit + Sigma.js (WebGL) frontend, a bun app
  rather than a cargo crate. Each animation frame it steps the wasm engine,
  copies the position buffer into graphology node attributes, and lets Sigma
  redraw. Nodes are colored by provider (derived from the snapshot `kind`
  prefix) and sized by degree; panels show live layout status, per-provider
  counts, observed traffic, and a reheat button.

  The `observations` overlay is *rendered*, not merely carried: a flow edge
  takes its verdict's color and a log-scaled width from its packet count,
  every node heard from gets a pulsing freshness halo, and packets animate
  along each flow edge as bright beads on a separate `canvas.traffic-layer`
  above Sigma's own (a bead in the edge's own hue disappears into a wide
  edge). Bead count and transit time are both log-scaled off `packets` —
  real volumes span orders of magnitude, so a linear mapping either
  saturates at the cap or leaves every flow at one bead.

## Build & test

```sh
# Rust unit tests (atlas-layout, atlas-layout-wasm) — run from THIS workspace;
# the root `cargo nextest run` does not reach it
cargo nextest run --all-targets

# Check wasm compilation without the JS glue
cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown --release
```

The web frontend uses [bun](https://bun.sh) (`wasm-pack` is a bun dev
dependency — no separate global install needed):

```sh
cd atlas-web
bun install
bun run wasm       # wasm-pack build → atlas-web/static/pkg/ (JS glue + .wasm)
bun dev            # http://localhost:4680
```

By default `bun dev` (i.e. `atlas-web`) connects live to `atlas-server` over
WebSocket (`ws://<host>:4681/ws`) — start that first, or use `?static` to fall
back to a one-shot `GET /snapshot.json`, which SvelteKit serves from
`atlas-web/static/snapshot.json`. Put a snapshot there to use that path (the
e2e `global-setup` copies `multi_cloud_demo.json` into it).

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
cargo run -p atlas-lib --example demo        # or: cargo xtask demo
# 2. Verify layout engine natively
cargo run --example layout_demo -- ../multi_cloud_demo.json   # inside atlas-render/
# 3. Serve it as the static fallback
cp ../multi_cloud_demo.json atlas-web/static/snapshot.json
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

`atlas-web/src/lib/GraphController.ts` is the real integration; the shape of
the loop:

```js
import init, { LayoutEngine } from "../../static/pkg/atlas_layout_wasm.js";

await init();
const engine = new LayoutEngine(await (await fetch("/snapshot.json")).text());
function frame() {
  engine.step(5);                              // physics budget per frame
  draw(engine.positionsView());                // zero-copy Float32Array
  if (engine.speed() > 0.01) requestAnimationFrame(frame);
}
frame();
```
