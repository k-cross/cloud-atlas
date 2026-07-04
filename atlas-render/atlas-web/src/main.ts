// Phase 2 of docs/graph_rendering_design.md: Sigma.js (WebGL) rendering fed
// by the Phase 1 wasm layout engine. The engine owns the physics; each
// animation frame we copy its interleaved position buffer into the
// graphology node attributes, which Sigma picks up through graph events.

import Sigma from "sigma";
import init, { LayoutEngine } from "../pkg/atlas_layout_wasm";
import { SNAPSHOT_VERSION, type Snapshot, buildGraph } from "./graph";
import { PROVIDER_COLORS, type Provider, providerOf } from "./style";

// Physics iterations per animation frame: the budget that keeps a frame
// under 16ms on large graphs while still converging in a few seconds.
const STEPS_PER_FRAME = 3;

// The engine's adaptive speed decays as the layout converges; below this
// the motion is invisible and we stop burning CPU.
const SETTLED_SPEED = 0.01;

const statusEl = document.getElementById("status")!;
const legendEl = document.getElementById("legend")!;
const errorEl = document.getElementById("error")!;

function fail(message: string): never {
  errorEl.textContent = message;
  errorEl.style.display = "grid";
  throw new Error(message);
}

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url);
  if (!response.ok) {
    fail(`GET ${url} failed (${response.status}) — did \`bun run wasm\` and the snapshot export run?`);
  }
  return response.text();
}

function renderLegend(snapshot: Snapshot) {
  const counts = new Map<Provider, number>();
  for (const node of snapshot.nodes) {
    const provider = providerOf(node.kind);
    counts.set(provider, (counts.get(provider) ?? 0) + 1);
  }
  legendEl.replaceChildren(
    ...[...counts.entries()]
      .sort((a, b) => b[1] - a[1])
      .map(([provider, count]) => {
        const row = document.createElement("div");
        const swatch = document.createElement("span");
        swatch.className = "swatch";
        swatch.style.background = PROVIDER_COLORS[provider];
        const label = document.createElement("span");
        label.textContent = `${provider} `;
        const n = document.createElement("span");
        n.className = "count";
        n.textContent = String(count);
        row.append(swatch, label, n);
        return row;
      }),
  );
}

async function main() {
  const [snapshotText] = await Promise.all([
    fetchText("/snapshot.json"),
    // The pkg/ glue resolves the wasm relative to import.meta.url, which
    // bundling rewrites — point it at the statically served file instead.
    init({ module_or_path: "/pkg/atlas_layout_wasm_bg.wasm" }),
  ]);

  const snapshot: Snapshot = JSON.parse(snapshotText);
  if (snapshot.version !== SNAPSHOT_VERSION) {
    fail(`snapshot version ${snapshot.version} != supported ${SNAPSHOT_VERSION}`);
  }

  const graph = buildGraph(snapshot);
  renderLegend(snapshot);

  const renderer = new Sigma(graph, document.getElementById("graph")!, {
    labelColor: { color: "#cfd6e4" },
    labelRenderedSizeThreshold: 7,
    minCameraRatio: 0.05,
    maxCameraRatio: 20,
  });

  let engine = new LayoutEngine(snapshotText);
  let iterations = 0;
  let running = false;

  function syncPositions() {
    // Re-acquire the view every frame: wasm memory growth invalidates it.
    const positions = engine.positionsView();
    let i = 0;
    graph.updateEachNodeAttributes(
      (_node, attrs) => {
        attrs.x = positions[2 * i];
        attrs.y = positions[2 * i + 1];
        i += 1;
        return attrs;
      },
      { attributes: ["x", "y"] },
    );
  }

  function updateStatus() {
    const state = running ? "layout running" : "settled";
    statusEl.textContent =
      `${graph.order} nodes · ${graph.size} edges · ` +
      `${iterations} iterations · ${state}`;
  }

  function frame() {
    engine.step(STEPS_PER_FRAME);
    iterations += STEPS_PER_FRAME;
    syncPositions();
    running = engine.speed() >= SETTLED_SPEED;
    updateStatus();
    if (running) requestAnimationFrame(frame);
  }

  function start() {
    if (!running) {
      running = true;
      requestAnimationFrame(frame);
    }
  }

  document.getElementById("reheat")!.addEventListener("click", () => {
    // The engine has no reset; a fresh instance re-runs the layout from the
    // deterministic initial placement.
    engine.free();
    engine = new LayoutEngine(snapshotText);
    iterations = 0;
    start();
  });

  syncPositions();
  start();
  return renderer;
}

main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
