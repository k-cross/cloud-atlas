import { execSync } from "node:child_process";
import { copyFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Prepare the credential-free assets the browser tests need:
//   1. the wasm layout engine (static/pkg), and
//   2. a deterministic snapshot at static/snapshot.json for `?static` mode.
// Runs once, before the webServers boot.
export default function globalSetup() {
	const here = dirname(fileURLToPath(import.meta.url)); // atlas-render/atlas-web/tests
	const web = resolve(here, "..");
	const repoRoot = resolve(here, "../../..");

	// Build the wasm engine if stale (fast no-op when fresh).
	execSync("cargo xtask wasm", { cwd: repoRoot, stdio: "inherit" });

	// Generate the Globex demo snapshot once, then serve it as the static fixture.
	const demo = resolve(repoRoot, "multi_cloud_demo.json");
	if (!existsSync(demo)) {
		execSync("cargo run -p atlas-lib --example demo", { cwd: repoRoot, stdio: "inherit" });
	}
	copyFileSync(demo, resolve(web, "static/snapshot.json"));
}
