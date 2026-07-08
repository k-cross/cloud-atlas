# atlas-web

Sigma.js (WebGL) frontend for the render snapshot, fed by the wasm layout
engine. See `../../docs/graph_rendering_design.md`.

## Develop

One command for the whole stack (wasm + live server + this frontend), from the
repo root:

```bash
cargo xtask dev --demo    # credential-free; Ctrl-C stops everything
```

Or run just this app by hand:

```bash
bun install
bun run wasm      # build pkg/ from atlas-layout-wasm
bun dev           # serve at http://localhost:4680
```

## Tests

Two layers guard against regressions:

### Unit (`bun test`) — fast, no build needed

Pure logic with no DOM or wasm dependency:

- `test/style.test.ts` — provider bucketing, color maps, degree→size scaling.
- `test/graph.test.ts` — snapshot/patch → graphology translation (`src/graph.ts`):
  stable-key node addressing, degree-based sizing, multigraph edge preservation,
  incremental `applyPatch` (add/remove, idempotent replay, out-of-order guard),
  and the `snapshotFromGraph` round-trip the layout engine is re-fed from.

```bash
bun run test      # == bun test ./test
bun test          # also fine — see note
```

> `bunfig.toml` pins the test root to `./test`, so a bare `bun test` won't try
> to run the Playwright `*.spec.ts` files under `e2e/` (which use a different
> runner and error out under bun's). Playwright discovers `e2e/` via its own
> `testDir` and is unaffected.

### End-to-end (`playwright test`) — full render pipeline

`e2e/render.spec.ts` runs headless Chromium against two `webServer`s Playwright
boots for it:

- **Static** — `serve.ts` hosts the credential-free Globex demo snapshot; the
  app loads it with `?static` (one HTTP fetch, no server) for deterministic
  render assertions: node/edge counts, the WebGL canvas, the provider legend,
  layout settling, the reheat path (wasm engine swap), and the
  snapshot-version-mismatch error overlay.
- **Live** — `atlas-server --demo` pushes a snapshot then churning patches over
  WebSocket; the app connects by default and the tests assert it renders the
  pushed snapshot and applies the live patches (node count changes as the demo
  sentinel flips).

**Prerequisites** (the Playwright `webServer`s won't start without them):

```bash
bun run wasm                              # pkg/ built (rebuild after any SNAPSHOT_VERSION bump)
cargo run --example demo                  # (repo root) writes multi_cloud_demo.json
bunx playwright install chromium          # one-time browser download
```

The live server is launched via `cargo run -p atlas-server` from the config, so
the root workspace must build. Then:

```bash
bun run test:e2e                          # == playwright test
```

Static assets serve on port 4680 (`E2E_PORT`) and the live server on 4681
(`E2E_SERVER_PORT`, the app's default WebSocket target), so neither collides
with a `bun dev` on 4680 if you reuse it.
