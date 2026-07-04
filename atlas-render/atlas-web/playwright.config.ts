import { defineConfig, devices } from "@playwright/test";

// E2E tests drive the real dev server (serve.ts) against the credential-free
// Globex demo snapshot, exercising the full pipeline: wasm layout engine +
// Sigma/WebGL rendering. Prerequisites (see README): `bun run wasm` has built
// pkg/, and multi_cloud_demo.json exists at the repo root (`cargo run
// --example demo`).
//
// Runs on a dedicated port so it never collides with a `bun dev` you have open.
const PORT = Number(process.env.E2E_PORT ?? 4681);

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "list" : [["list"]],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: "on-first-retry",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    command: "bun serve.ts ../../multi_cloud_demo.json",
    url: `http://localhost:${PORT}`,
    env: { PORT: String(PORT) },
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
    stderr: "pipe",
  },
});
