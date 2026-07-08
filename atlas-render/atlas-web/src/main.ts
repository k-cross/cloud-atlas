// Phase 2 of docs/graph_rendering_design.md, now fed live: the frontend opens
// a WebSocket to atlas-server, renders the initial snapshot, and applies the
// incremental patches the server pushes as the graph changes. The wasm layout
// engine (Phase 1) still owns the physics; on every topology change we hand it
// the updated graph and let it re-converge. Falls back to a static
// `/snapshot.json` fetch when no server is reachable (bun `serve.ts`).

import Graph from "graphology";
import Sigma from "sigma";
import init, { LayoutEngine } from "../pkg/atlas_layout_wasm";
import {
  type GraphPatch,
  SNAPSHOT_VERSION,
  type Snapshot,
  applyPatch,
  buildGraph,
  snapshotFromGraph,
} from "./graph";
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

// Sigma throws "Container has no width" if the element is 0-sized at
// construction. A full-viewport `position:absolute; inset:0` div reads
// offsetWidth 0 until the browser lays it out — which it defers for a
// background tab or a not-yet-shown window. Resolve as soon as the container
// has real dimensions (immediately in the common case).
function whenSized(el: HTMLElement): Promise<void> {
  if (el.offsetWidth > 0 && el.offsetHeight > 0) return Promise.resolve();
  return new Promise((resolve) => {
    const observer = new ResizeObserver(() => {
      if (el.offsetWidth > 0 && el.offsetHeight > 0) {
        observer.disconnect();
        resolve();
      }
    });
    observer.observe(el);
  });
}

function fail(message: string): never {
  errorEl.textContent = message;
  errorEl.style.display = "grid";
  throw new Error(message);
}

// Server URL is overridable (`?server=ws://host:port/ws`); default targets the
// atlas-server dev port on the same host.
function serverUrl(): string {
  const override = new URLSearchParams(location.search).get("server");
  if (override) return override;
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${location.hostname}:4681/ws`;
}

function renderLegend(graph: Graph) {
  const counts = new Map<Provider, number>();
  graph.forEachNode((_key, attrs) => {
    const provider = providerOf(attrs.kind as string);
    counts.set(provider, (counts.get(provider) ?? 0) + 1);
  });
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

function checkVersion(version: number) {
  if (version !== SNAPSHOT_VERSION) {
    fail(`snapshot version ${version} != supported ${SNAPSHOT_VERSION}`);
  }
}

async function main() {
  const container = document.getElementById("graph")!;
  // Don't build Sigma into a 0-width container; wait for layout first.
  await Promise.all([
    init({ module_or_path: "/pkg/atlas_layout_wasm_bg.wasm" }),
    whenSized(container),
  ]);

  // One long-lived graph instance backs Sigma; snapshots refill it and patches
  // mutate it in place, so Sigma's WebGL buffers update through graph events.
  const graph = new Graph({ multi: true, type: "directed" });
  const renderer = new Sigma(graph, container, {
    labelColor: { color: "#cfd6e4" },
    labelRenderedSizeThreshold: 7,
    minCameraRatio: 0.05,
    maxCameraRatio: 20,
    // Belt-and-suspenders: if a 0-size container ever slips past whenSized,
    // Sigma constructs anyway and its own ResizeObserver refreshes on resize.
    allowInvalidContainer: true,
  });

  let engine: LayoutEngine | null = null;
  let iterations = 0;
  let running = false;
  let connection = "connecting";

  function syncPositions() {
    if (!engine) return;
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
      `${iterations} iterations · ${state} · ${connection}`;
  }

  function frame() {
    if (!engine) return;
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

  // (Re)build the layout engine from the current graph. The engine has no
  // incremental API, so any topology change re-runs the layout from the
  // deterministic initial placement — same mechanism as the reheat button.
  function restartEngine() {
    engine?.free();
    engine = new LayoutEngine(JSON.stringify(snapshotFromGraph(graph)));
    iterations = 0;
    syncPositions();
    start();
  }

  function loadSnapshot(snapshot: Snapshot) {
    checkVersion(snapshot.version);
    graph.clear();
    graph.import(buildGraph(snapshot).export());
    restartEngine();
    renderLegend(graph);
  }

  function onPatch(patch: GraphPatch) {
    checkVersion(patch.version);
    applyPatch(graph, patch);
    restartEngine();
    renderLegend(graph);
  }

  document.getElementById("reheat")!.addEventListener("click", restartEngine);

  const handlers: LiveHandlers = {
    onSnapshot: loadSnapshot,
    onPatch,
    onStatus: (s) => {
      connection = s;
      updateStatus();
    },
  };

  // `?static` forces the one-shot snapshot fetch (no server) — used for static
  // hosting and deterministic tests. Otherwise go live over WebSocket.
  if (new URLSearchParams(location.search).has("static")) {
    void fetchStatic(handlers);
  } else {
    const live = connectLive(handlers);
    // Clicking a node pulls its neighborhood from the server — the client
    // asking for specific data on demand, not just listening. Graphology node
    // keys are the stable snapshot keys, so `node` is what the server expects.
    renderer.on("clickNode", ({ node }) => live.requestNeighbors(node));
  }

  return renderer;
}

interface LiveHandlers {
  onSnapshot: (s: Snapshot) => void;
  onPatch: (p: GraphPatch) => void;
  onStatus: (s: string) => void;
}

interface LiveConnection {
  requestNeighbors: (key: string) => void;
}

// Connect to atlas-server over WebSocket; on failure, fall back to the static
// snapshot so `serve.ts`-style static hosting still works.
function connectLive(handlers: LiveHandlers): LiveConnection {
  const url = serverUrl();
  let gotMessage = false;

  const socket = new WebSocket(url);

  socket.addEventListener("open", () => {
    handlers.onStatus(`live: ${url}`);
    // Explicit subscribe is optional (the server pushes a snapshot on connect)
    // but documents intent and re-triggers a snapshot on demand.
    socket.send(JSON.stringify({ type: "subscribe" }));
  });

  socket.addEventListener("message", (ev) => {
    gotMessage = true;
    const msg = JSON.parse(ev.data);
    switch (msg.type) {
      case "snapshot":
        handlers.onSnapshot(msg as Snapshot);
        break;
      case "patch":
        handlers.onPatch(msg as GraphPatch);
        break;
      case "neighbors":
        console.info(`neighbors of ${msg.key}:`, msg.nodes, msg.edges);
        break;
      case "error":
        console.warn("server error:", msg.message);
        break;
    }
  });

  socket.addEventListener("close", () => {
    handlers.onStatus(gotMessage ? "disconnected" : "offline");
    if (!gotMessage) fetchStatic(handlers);
  });

  // `error` fires just before `close`, which handles the static fallback.
  socket.addEventListener("error", () => {});

  return {
    requestNeighbors(key) {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: "get_neighbors", key }));
      }
    },
  };
}

async function fetchStatic(handlers: LiveHandlers) {
  try {
    const res = await fetch("/snapshot.json");
    if (!res.ok) {
      fail(
        `no live server and GET /snapshot.json failed (${res.status}) — start ` +
          `atlas-server (cargo run -p atlas-server -- --demo) or generate a static snapshot.`,
      );
    }
    handlers.onStatus("static");
    handlers.onSnapshot((await res.json()) as Snapshot);
  } catch (e) {
    fail(e instanceof Error ? e.message : String(e));
  }
}

main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
