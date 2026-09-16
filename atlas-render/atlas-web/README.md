# atlas-web

SvelteKit SPA (SSR off — WebGL needs the browser) rendering the live graph with Sigma.js.

- **Layout**: `atlas-layout-wasm`, built into `static/pkg/` by `bun run wasm` and imported by `GraphController.ts`.
- **Data**: WebSocket to `atlas-server` (snapshot, then patches). With no server, or `?static`, it fetches `static/snapshot.json` once.
- **Overlay**: `observations` drive flow-edge colour/width, node freshness halos, and packet beads on `canvas.traffic-layer` (`lib/traffic.ts`, `lib/TrafficLayer.ts`). Derived `Serves` edges are coloured by status and the service view hides everything else (`lib/service.ts`).

## Development

```sh
cargo xtask dev --demo   # wasm + atlas-server + this app, from the repo root
```

Standalone:

```sh
bun install
bun run wasm
bun run dev
```

## Testing

- `bun run test:unit` — pure logic in `src/lib/*.test.ts` (no DOM): snapshot/patch application, styling, traffic and service-view helpers.
- `bun run test:e2e` — Playwright/Chromium against the real pipeline, both `?static` and live against `atlas-server --demo`: counts, legend, layout settling and warm-start pinning, the settled-graph pixel-stability (shake) regression, patches, the service view, and version-mismatch handling. The traffic canvas is excluded from the stability check; a separate test asserts it animates.
- `bun run test` — `bun run wasm` then Playwright.

`cargo xtask test` runs lint, `svelte-check` and unit tests (plus e2e with `--e2e`).

## Formatting & linting

[Biome](https://biomejs.dev) handles JS/TS/JSON/CSS; `.svelte` files belong to the Svelte tooling (`bun run check`).

```sh
bun run format     # biome format --write .
bun run lint       # biome check .
bun run lint:fix   # biome check --write .
```
