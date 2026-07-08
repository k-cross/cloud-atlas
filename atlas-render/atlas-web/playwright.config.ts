import { defineConfig, devices } from "@playwright/test";

// E2E tests exercise the full pipeline (wasm layout engine + Sigma/WebGL) two
// ways:
//   - Static: serve.ts hosts the Globex demo snapshot; the app loads it with
//     `?static` for deterministic, server-independent render assertions.
//   - Live: atlas-server (`--demo`) pushes a snapshot then churning patches
//     over WebSocket; the app connects by default and applies them.
//
// Prerequisites (see README): `bun run wasm` has built pkg/, and
// multi_cloud_demo.json exists at the repo root (`cargo run --example demo`).
//
// Dedicated ports so nothing collides with a `bun dev` you have open. The app's
// default WebSocket target is port 4681, so the live server must listen there.
const ASSET_PORT = Number(process.env.E2E_PORT ?? 4680);
const SERVER_PORT = Number(process.env.E2E_SERVER_PORT ?? 4681);

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "list" : [["list"]],
  use: {
    baseURL: `http://localhost:${ASSET_PORT}`,
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: [
    {
      command: "bun serve.ts ../../multi_cloud_demo.json",
      url: `http://localhost:${ASSET_PORT}`,
      env: { PORT: String(ASSET_PORT) },
      reuseExistingServer: !process.env.CI,
      stdout: "pipe",
      stderr: "pipe",
    },
    {
      // atlas-server lives in the root workspace, not atlas-render's — point
      // cargo at it explicitly. A fast poll makes the churn visible quickly.
      command: `cargo run --manifest-path ../../Cargo.toml -p atlas-server -- --demo --poll-secs 2 --port ${SERVER_PORT}`,
      url: `http://localhost:${SERVER_PORT}/snapshot.json`,
      reuseExistingServer: !process.env.CI,
      timeout: 180_000,
      stdout: "pipe",
      stderr: "pipe",
    },
  ],
});
