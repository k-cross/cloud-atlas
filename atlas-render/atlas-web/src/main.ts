// Phase 2 of docs/graph_rendering_design.md, now fed live: the frontend opens
// a WebSocket to atlas-server, renders the initial snapshot, and applies the
// incremental patches the server pushes as the graph changes. Falls back to a
// static `/snapshot.json` fetch when no server is reachable (bun `serve.ts`).
//
// The wasm layout engine (Phase 1) settles each layout *off-screen*; only the
// converged result is revealed (with a short tween under a pinned coordinate
// box). Streaming the engine's raw per-iteration positions into Sigma — plus
// Sigma re-fitting the view on every change — was what made the graph shake.

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

// The layout is settled *off-screen* — the engine's raw per-iteration churn is
// never streamed into Sigma (that churn, plus Sigma re-normalizing the view on
// every position change, was the shaking). Only the final settled layout is
// revealed, via a short eased tween, under a pinned coordinate box.
//
// Settling is time-budgeted per frame so a large graph never blocks the main
// thread; the engine's adaptive speed tells us when it has converged.
const SETTLE_BUDGET_MS = 10; // work per frame while settling
const SETTLE_STEP = 20; // engine iterations per inner step
const SETTLE_MAX_ITERS = 3000; // safety cap for pathological graphs
const SETTLED_SPEED = 0.01; // engine speed below which we call it settled
const REVEAL_MS = 700; // eased reveal (tween to the settled layout) duration
const BBOX_PADDING = 0.08; // fraction of the extent added around the layout

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
  let phase = "loading"; // loading | laying out | settled
  let connection = "connecting";
  let anim = 0; // in-flight rAF id (settle or reveal), cancellable
  let bboxPinned = false; // coordinate box fixed after the first cold layout

  function updateStatus() {
    statusEl.textContent =
      `${graph.order} nodes · ${graph.size} edges · ` +
      `${iterations} iterations · ${phase} · ${connection}`;
  }

  const easeOutCubic = (t: number) => 1 - (1 - t) ** 3;

  // Pin Sigma's coordinate normalization to a fixed box. Sigma otherwise
  // recomputes the graph extent on every position change and re-fits the whole
  // view each frame, so any node moving makes the entire graph rescale/recenter
  // — the shaking, amplified under zoom. A fixed box makes screen positions a
  // stable function of graph coordinates.
  function pinBBox(positions: Float32Array) {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (let i = 0; i < positions.length; i += 2) {
      minX = Math.min(minX, positions[i]);
      maxX = Math.max(maxX, positions[i]);
      minY = Math.min(minY, positions[i + 1]);
      maxY = Math.max(maxY, positions[i + 1]);
    }
    const padX = (maxX - minX) * BBOX_PADDING || 1;
    const padY = (maxY - minY) * BBOX_PADDING || 1;
    renderer.setCustomBBox({
      x: [minX - padX, maxX + padX],
      y: [minY - padY, maxY + padY],
    });
    renderer.refresh();
  }

  // Current graph coordinates, in graphology iteration order (== engine order).
  function currentPositions(): { x: number[]; y: number[] } {
    const x: number[] = [];
    const y: number[] = [];
    graph.forEachNode((_k, a) => {
      x.push(a.x as number);
      y.push(a.y as number);
    });
    return { x, y };
  }

  // Ease every node from its current position to the settled `target`, holding
  // the pinned box fixed so the tween is pure motion with no re-normalization.
  function reveal(start: { x: number[]; y: number[] }, target: Float32Array) {
    if (anim) cancelAnimationFrame(anim);
    const t0 = performance.now();
    const tick = () => {
      const e = easeOutCubic(Math.min(1, (performance.now() - t0) / REVEAL_MS));
      let i = 0;
      graph.updateEachNodeAttributes(
        (_k, a) => {
          a.x = start.x[i] + (target[2 * i] - start.x[i]) * e;
          a.y = start.y[i] + (target[2 * i + 1] - start.y[i]) * e;
          i += 1;
          return a;
        },
        { attributes: ["x", "y"] },
      );
      if (e < 1) {
        anim = requestAnimationFrame(tick);
      } else {
        phase = "settled";
        anim = 0;
      }
      updateStatus();
    };
    anim = requestAnimationFrame(tick);
  }

  // Rebuild the engine from the current graph, settle it off-screen, then
  // reveal the result. `warm` seeds the engine with current positions (via the
  // snapshot) so a live patch continues the layout locally; cold (initial load,
  // reheat) lays out afresh, re-pins the box, and refits the camera.
  function layoutAndReveal(warm: boolean) {
    if (anim) cancelAnimationFrame(anim);
    engine?.free();
    engine = new LayoutEngine(JSON.stringify(snapshotFromGraph(graph, warm)));
    if (!warm) iterations = 0;

    const start = currentPositions();
    phase = "laying out";
    let settleIters = 0;

    const settle = () => {
      const budgetStart = performance.now();
      // Run as many steps as fit in the frame budget — fast graphs settle in a
      // frame or two; big graphs stay responsive.
      while (
        performance.now() - budgetStart < SETTLE_BUDGET_MS &&
        engine!.speed() >= SETTLED_SPEED &&
        settleIters < SETTLE_MAX_ITERS
      ) {
        engine!.step(SETTLE_STEP);
        settleIters += SETTLE_STEP;
        iterations += SETTLE_STEP;
      }
      updateStatus();
      if (engine!.speed() >= SETTLED_SPEED && settleIters < SETTLE_MAX_ITERS) {
        anim = requestAnimationFrame(settle);
        return;
      }
      const target = engine!.positionsCopy(); // detached from wasm memory
      if (!warm || !bboxPinned) {
        pinBBox(target);
        bboxPinned = true;
        renderer.getCamera().animatedReset(); // fit the freshly pinned box
      }
      reveal(start, target);
    };
    anim = requestAnimationFrame(settle);
  }

  function loadSnapshot(snapshot: Snapshot) {
    checkVersion(snapshot.version);
    graph.clear();
    graph.import(buildGraph(snapshot).export());
    layoutAndReveal(false);
    renderLegend(graph);
  }

  function onPatch(patch: GraphPatch) {
    checkVersion(patch.version);
    applyPatch(graph, patch);
    // Warm: keep existing nodes where they are and settle the change locally,
    // so a live update nudges the graph instead of relaying it out.
    layoutAndReveal(true);
    renderLegend(graph);
  }

  // Reheat is a deliberate cold re-layout of the whole graph.
  document
    .getElementById("reheat")!
    .addEventListener("click", () => layoutAndReveal(false));

  // Debug/introspection handle: the live graphology graph and Sigma renderer,
  // handy from the console (e.g. reading node positions) and used by the e2e
  // tests.
  (window as unknown as { atlas: unknown }).atlas = { graph, renderer };

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
