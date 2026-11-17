# atlas-web

SvelteKit SPA (SSR disabled for WebGL) rendering the live graph with Sigma.js.

- **Layout:** `atlas-layout-wasm` built into `static/pkg/` (`bun run wasm`), driven by `GraphController.ts`.
- **Data:** Connects to `atlas-server` (`ws://<host>:4681/ws`, override via `?server=ws://…`) for initial snapshot and live patches; falls back to `static/snapshot.json` when offline or with `?static`.
- **Overlay & views:** `observations` drive flow-edge colour/width, node freshness halos, and animated packet beads on `canvas.traffic-layer` (`lib/traffic.ts`, `lib/TrafficLayer.ts`). Derived `Serves` edges style by status, and the service view hides non-service topology (`lib/service.ts`).

## Commands

```sh
# From repo root
cargo xtask dev --demo
cargo xtask test [--e2e]

# Standalone (in atlas-web/)
bun install && bun run wasm && bun run dev
bun run test:unit    # unit tests in src/lib/*.test.ts (no DOM)
bun run test:e2e     # Playwright against ?static and live atlas-server --demo
bun run check        # svelte-check
bun run lint         # biome check . (use lint:fix / format to write)
```
