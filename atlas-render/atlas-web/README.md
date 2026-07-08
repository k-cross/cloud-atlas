# atlas-web

The web frontend for Cloud Atlas. This is a SvelteKit Single Page Application (SPA) that renders the live digital twin cloud environment graph using WebGL.

## Architecture

- **SvelteKit**: Manages the application structure, routing, and sleek UI overlay. (Server-Side Rendering is intentionally disabled since the core visualization requires browser-native WebGL contexts).
- **Sigma.js**: Renders the large-scale property graph dynamically using WebGL.
- **atlas-layout-wasm**: A WebAssembly port of the backend's ForceAtlas2 physics layout engine. Computes complex node forces directly in the browser for high performance.
- **WebSocket**: Connects to `atlas-server` to ingest continuous live snapshots and incremental graph patches from the cloud environment.

## Development

The frontend is tightly integrated with the workspace's `xtask` orchestrator.

**Run the development stack:**
```sh
cargo xtask dev --demo
```
This command automatically:
1. Compiles the Rust WebAssembly layout engine.
2. Boots the SvelteKit development server (`bun run dev`).
3. Boots the live `atlas-server` backend.

**Run the frontend independently:**
If you need to isolate the frontend development process:
```sh
# Install dependencies
bun install

# Build the WebAssembly engine
bun run wasm

# Run SvelteKit in dev mode
bun run dev
```

## Testing

Two layers:

- **Unit** (`bun run test:unit`) — pure logic in `src/lib/*.test.ts` (Bun test runner, no DOM): snapshot→graphology translation, incremental `applyPatch`, `snapshotFromGraph` round-trip, provider bucketing, degree sizing.
- **End-to-end** (`bun run test:e2e`) — Playwright/Chromium against the real pipeline (SvelteKit + wasm layout + Sigma/WebGL), both data paths: `?static` (a fixture `global-setup` writes) and live over WebSocket against `atlas-server --demo`. Covers node/edge counts, the WebGL canvas, the legend, layout settling, reheat, the snapshot-version-mismatch overlay, the zero-width-container guard, the **settled-graph pixel-stability** (shake) regression, live patch application, and **warm-start node pinning**.

```sh
bun run test:unit   # fast, no build
bun run test:e2e    # global-setup builds wasm + snapshot fixture, then two webServers
bun run test        # bun run wasm && playwright test
```

Both layers run in the workspace gate (`cargo xtask test`).

## Formatting & Linting

[Biome](https://biomejs.dev) is the single tool for formatting and linting JS/TS/JSON/CSS (preferred over Prettier/ESLint). `.svelte` files are owned by the Svelte tooling (`bun run check` / the Svelte VS Code extension), since Biome only sees a `.svelte` file's `<script>` in isolation.

```sh
bun run format     # biome format --write .
bun run lint       # biome check .            (format + lint + import sorting; no writes)
bun run lint:fix   # biome check --write .    (apply safe fixes)
```

`cargo xtask test` runs `bun run lint` as part of the gate, and `.vscode/` sets Biome as the default formatter with format-on-save.
