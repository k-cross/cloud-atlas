# atlas-web

Sigma.js (WebGL) frontend for the render snapshot, fed by the wasm layout
engine. See `../../docs/graph_rendering_design.md`.

## Develop

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
- `test/graph.test.ts` — snapshot → graphology translation (`src/graph.ts`):
  sparse-id re-keying, degree-based sizing, multigraph edge preservation, the
  cross-cloud seam edge colors.

```bash
bun run test      # == bun test ./test
bun test          # also fine — see note
```

> `bunfig.toml` pins the test root to `./test`, so a bare `bun test` won't try
> to run the Playwright `*.spec.ts` files under `e2e/` (which use a different
> runner and error out under bun's). Playwright discovers `e2e/` via its own
> `testDir` and is unaffected.

### End-to-end (`playwright test`) — full render pipeline

`e2e/render.spec.ts` drives the real dev server (`serve.ts`) in headless
Chromium against the credential-free Globex demo snapshot, exercising wasm
layout + Sigma/WebGL end to end: node/edge counts, the WebGL canvas, the
provider legend, layout settling, the reheat path (wasm engine swap), and the
snapshot-version-mismatch error overlay.

**Prerequisites** (the Playwright `webServer` won't start without them):

```bash
bun run wasm                              # pkg/ built
cargo run --example demo                  # (repo root) writes multi_cloud_demo.json
bunx playwright install chromium          # one-time browser download
```

Then:

```bash
bun run test:e2e                          # == playwright test
```

Runs on port 4681 (override with `E2E_PORT`) so it never collides with a
`bun dev` on 4680.
