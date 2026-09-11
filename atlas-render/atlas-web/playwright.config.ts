import { defineConfig, devices } from "@playwright/test";

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
			command: `cargo run --manifest-path ../../Cargo.toml -p atlas-server -- --demo --poll-secs 2 --port ${SERVER_PORT}`,
			url: `http://localhost:${SERVER_PORT}/snapshot.json`,
			reuseExistingServer: !process.env.CI,
			timeout: 180_000,
			stdout: "pipe",
			stderr: "pipe",
		},
	],
});
