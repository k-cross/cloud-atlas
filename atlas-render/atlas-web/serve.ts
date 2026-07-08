// Dev server: Bun bundles index.html (and its TypeScript) on the fly and we
// add two data routes on top — the render snapshot and the wasm-pack output.
//
//   bun serve.ts [path/to/snapshot.json]
//
// Without an argument it serves the most likely snapshot from the repo root:
// `atlas.json` (real collection) or `multi_cloud_demo.json` (Globex demo).

import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import index from "./index.html";

const arg = Bun.argv[2];
const fallbacks = ["../../atlas.json", "../../multi_cloud_demo.json"].map((p) =>
  resolve(import.meta.dir, p),
);
const snapshotPath = arg ? resolve(arg) : fallbacks.find(existsSync);

// A static snapshot is optional: the app is WebSocket-first and only fetches
// /snapshot.json as the offline (`?static`) fallback. Warn instead of exiting
// so live-mode dev works before any snapshot file has ever been written.
if (!snapshotPath || !existsSync(snapshotPath)) {
  console.warn(
    "no static render snapshot found — /snapshot.json will 404 (live WebSocket mode still works).\n" +
      "To enable the ?static fallback, generate one:\n" +
      "  cargo run --example demo        (repo root, credential-free)\n" +
      "  cargo run -- --regions ...      (real collection)\n" +
      "or pass a path: bun serve.ts path/to/snapshot.json",
  );
}

if (!existsSync(join(import.meta.dir, "pkg", "atlas_layout_wasm.js"))) {
  console.error("pkg/ missing — run `bun run wasm` first.");
  process.exit(1);
}

const port = Number(process.env.PORT ?? 4680);

const routes = {
  "/": index,
  "/snapshot.json": () =>
    snapshotPath && existsSync(snapshotPath)
      ? new Response(Bun.file(snapshotPath))
      : new Response("no static snapshot available", { status: 404 }),
  "/pkg/:file": async (req: Bun.BunRequest<"/pkg/:file">) => {
    const name = req.params.file;
    if (name.includes("/") || name.includes("..")) {
      return new Response("bad path", { status: 400 });
    }
    const file = Bun.file(join(import.meta.dir, "pkg", name));
    if (!(await file.exists())) {
      return new Response("not found", { status: 404 });
    }
    return new Response(file);
  },
};

let server;
try {
  server = Bun.serve({ port, development: true, routes });
} catch (e) {
  if (e instanceof Error && "code" in e && e.code === "EADDRINUSE") {
    console.error(
      `port ${port} is already in use — another dev server is likely still running.\n` +
        `Free it with:  lsof -ti:${port} | xargs kill\n` +
        `or serve elsewhere:  PORT=4681 bun dev`,
    );
    process.exit(1);
  }
  throw e;
}

console.log(
  `atlas-web: serving ${snapshotPath ?? "(no static snapshot — live mode only)"} at ${server.url}`,
);
