import { execSync } from "node:child_process";
import { copyFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export default function globalSetup() {
	const here = dirname(fileURLToPath(import.meta.url));
	const web = resolve(here, "..");
	const repoRoot = resolve(here, "../../..");

	execSync("cargo xtask wasm", { cwd: repoRoot, stdio: "inherit" });

	const demo = resolve(repoRoot, "multi_cloud_demo.json");
	execSync("cargo run -p atlas-lib --example demo", { cwd: repoRoot, stdio: "inherit" });
	copyFileSync(demo, resolve(web, "static/snapshot.json"));
}
