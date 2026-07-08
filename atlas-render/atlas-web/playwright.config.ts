import { defineConfig, devices } from "@playwright/test";

// E2E exercises the full pipeline (SvelteKit + wasm layout + Sigma/WebGL) two
// ways, mirroring the app's two data paths:
//   - Static: the app loads `/snapshot.json` (a fixture written by
//     global-setup) with `?static` — deterministic, server-independent.
//   - Live: atlas-server (`--demo`) pushes a snapshot then churning patches
//     over WebSocket; the app connects by default and applies them.
//
// global-setup builds the wasm engine and writes the snapshot fixture. The app
// (vite dev) serves on 4680; the app's default WebSocket target is 4681, so the
// live server must listen there.
const APP_PORT = Number(process.env.E2E_PORT ?? 4680);
const SERVER_PORT = Number(process.env.E2E_SERVER_PORT ?? 4681);

export default defineConfig({
	testDir: "tests",
	testMatch: /(.+\.)?(test|spec)\.[jt]s/,
	globalSetup: "./tests/global-setup.ts",
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	use: {
		baseURL: `http://localhost:${APP_PORT}`,
		trace: "on-first-retry",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	webServer: [
		{
			command: "bun run dev",
			url: `http://localhost:${APP_PORT}`,
			reuseExistingServer: !process.env.CI,
			stdout: "pipe",
			stderr: "pipe",
		},
		{
			// atlas-server lives in the root workspace, not atlas-render's.
			command: `cargo run --manifest-path ../../Cargo.toml -p atlas-server -- --demo --poll-secs 2 --port ${SERVER_PORT}`,
			url: `http://localhost:${SERVER_PORT}/snapshot.json`,
			reuseExistingServer: !process.env.CI,
			timeout: 180_000,
			stdout: "pipe",
			stderr: "pipe",
		},
	],
});
