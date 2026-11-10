import Graph from "graphology";
import Sigma from "sigma";
import type { EdgeDisplayData, NodeDisplayData } from "sigma/types";
import init, { LayoutEngine } from "../../static/pkg/atlas_layout_wasm.js";
import {
	applyPatch,
	buildGraph,
	changesTopology,
	type GraphPatch,
	SNAPSHOT_VERSION,
	type Snapshot,
	snapshotFromGraph,
} from "./graph";
import type { CollectionHealth } from "./health";
import { emptySearch, type SearchResult, searchNodes } from "./search";
import { SERVES_KIND, type ServiceSummary, serviceEndpoints, serviceSummary } from "./service";
import {
	DIMMED_EDGE_COLOR,
	DIMMED_NODE_COLOR,
	PROVIDER_COLORS,
	type Provider,
	providerOf,
} from "./style";
import { TrafficLayer } from "./TrafficLayer";
import type { TrafficSummary } from "./traffic";

const SETTLE_BUDGET_MS = 10;
const SETTLE_STEP = 20;
const SETTLE_MAX_ITERS = 3000;
const SETTLED_SPEED = 0.01;
const REVEAL_MS = 700;
const BBOX_PADDING = 0.08;
const FOCUS_RATIO = 0.3;
const FOCUS_MS = 600;
const FORCED_LABELS = 24;

const easeOutCubic = (t: number) => 1 - (1 - t) ** 3;

function errorText(e: unknown): string {
	return e instanceof Error ? e.message : String(e);
}

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

function serverUrl(): string {
	const override = new URLSearchParams(location.search).get("server");
	if (override) return override;
	const scheme = location.protocol === "https:" ? "wss" : "ws";
	return `${scheme}://${location.hostname}:4681/ws`;
}

function checkVersion(version: number) {
	if (version !== SNAPSHOT_VERSION) {
		throw new Error(`snapshot version ${version} != supported ${SNAPSHOT_VERSION}`);
	}
}

export type LegendCount = { provider: Provider; color: string; count: number };

export interface GraphControllerOptions {
	container: HTMLElement;
	onStatusChange: (statusText: string) => void;
	onLegendChange: (legend: LegendCount[]) => void;
	onTrafficChange: (traffic: TrafficSummary) => void;
	onServiceChange: (service: ServiceSummary) => void;
	onHealthChange: (health: CollectionHealth | null) => void;
	onSearchChange: (search: SearchResult) => void;
	onFocusChange: (key: string | null, label: string | null) => void;
	onError: (error: string) => void;
}

export class GraphController {
	private graph: Graph;
	private renderer: Sigma;
	private engine: LayoutEngine | null = null;
	private iterations = 0;
	private phase = "loading";
	private connection = "connecting";
	private anim = 0;
	private laidOut = false;
	private options: GraphControllerOptions;
	private socket: WebSocket | null = null;
	private traffic: TrafficLayer | null = null;
	private serviceOnly: Set<string> | null = null;
	private searchResult: SearchResult = emptySearch();
	private focused: string | null = null;
	private focusSet: Set<string> | null = null;

	constructor(options: GraphControllerOptions) {
		this.options = options;
		this.graph = new Graph({ multi: true, type: "directed" });
		this.renderer = new Sigma(this.graph, options.container, {
			labelColor: { color: "#cfd6e4" },
			labelRenderedSizeThreshold: 7,
			minCameraRatio: 0.05,
			maxCameraRatio: 20,
			allowInvalidContainer: true,
			zIndex: true,
			nodeReducer: (key, data) => this.reduceNode(key, data),
			edgeReducer: (key, data) => this.reduceEdge(key, data),
		});
	}

	public async initialize() {
		try {
			await Promise.all([
				init({ module_or_path: "/pkg/atlas_layout_wasm_bg.wasm" }),
				whenSized(this.options.container),
			]);

			this.traffic = new TrafficLayer(
				this.renderer,
				this.graph,
				this.options.onTrafficChange,
				(key) => this.dimmed(key),
				(source, target) => this.edgeDimmed(source, target),
			);
			this.traffic.start();

			(window as unknown as { atlas: unknown }).atlas = {
				graph: this.graph,
				renderer: this.renderer,
			};

			this.renderer.on("clickNode", ({ node }) => {
				this.focus(node);
				this.requestNeighbors(node);
			});
			this.renderer.on("clickStage", () => this.focus(null));

			if (new URLSearchParams(location.search).has("static")) {
				await this.fetchStatic();
			} else {
				this.connectLive();
			}
		} catch (e) {
			this.options.onError(errorText(e));
		}
	}

	private updateStatus() {
		const text = `${this.graph.order} nodes · ${this.graph.size} edges · ${this.iterations} iterations · ${this.phase} · ${this.connection}`;
		this.options.onStatusChange(text);
	}

	// Hiding the plumbing rather than dimming it is what makes the path legible:
	// a Serves edge summarises five hops, and drawing those hops underneath it
	// is the picture the derived edge was meant to replace. The traffic canvas
	// follows for free — it skips any endpoint Sigma reports as hidden, so the
	// packet beads stop with the flow edges they belong to.
	public setServiceView(on: boolean) {
		this.serviceOnly = on ? serviceEndpoints(this.graph) : null;
		this.refreshSearch();
	}

	private hiddenInServiceView(key: string): boolean {
		return this.serviceOnly !== null && !this.serviceOnly.has(key);
	}

	// Dimmed rather than hidden, so the matches keep their place in the shape of
	// the estate.
	private litNodes(): Set<string> | null {
		if (this.focusSet !== null) return this.focusSet;
		return this.searchResult.query.trim() ? this.searchResult.matches : null;
	}

	private dimmed(key: string): boolean {
		const lit = this.litNodes();
		return lit !== null && !lit.has(key);
	}

	private reduceNode(key: string, data: Partial<NodeDisplayData>): Partial<NodeDisplayData> {
		if (this.hiddenInServiceView(key)) return { ...data, hidden: true };
		const lit = this.litNodes();
		if (lit === null) return data;
		if (!lit.has(key)) return { ...data, color: DIMMED_NODE_COLOR, label: "", zIndex: 0 };
		return {
			...data,
			zIndex: 1,
			forceLabel: this.focusSet !== null && lit.size <= FORCED_LABELS,
			highlighted: key === this.focused,
		};
	}

	private edgeDimmed(source: string, target: string): boolean {
		const lit = this.litNodes();
		if (lit === null) return false;
		if (this.focused !== null) return source !== this.focused && target !== this.focused;
		return !lit.has(source) || !lit.has(target);
	}

	private reduceEdge(
		key: string,
		data: Partial<EdgeDisplayData> & { kind?: string },
	): Partial<EdgeDisplayData> {
		if (this.serviceOnly && data.kind !== SERVES_KIND) return { ...data, hidden: true };
		if (this.litNodes() === null) return data;
		const [source, target] = this.graph.extremities(key);
		return this.edgeDimmed(source, target)
			? { ...data, color: DIMMED_EDGE_COLOR, zIndex: 0 }
			: { ...data, zIndex: 1 };
	}

	private neighbourhood(key: string): Set<string> {
		return new Set([key, ...this.graph.neighbors(key)]);
	}

	public setQuery(query: string) {
		this.searchResult = searchNodes(this.graph, query, (key) => !this.hiddenInServiceView(key));
		this.renderer.refresh();
		this.options.onSearchChange(this.searchResult);
	}

	public focus(key: string | null) {
		const target =
			key !== null && this.graph.hasNode(key) && !this.hiddenInServiceView(key) ? key : null;
		this.focused = target;
		this.focusSet = target === null ? null : this.neighbourhood(target);
		this.renderer.refresh();
		this.options.onFocusChange(
			target,
			target === null ? null : (this.graph.getNodeAttribute(target, "label") as string),
		);
		if (target === null) return;

		const position = this.renderer.getNodeDisplayData(target);
		const camera = this.renderer.getCamera();
		if (position) {
			camera.animate(
				{ x: position.x, y: position.y, ratio: Math.min(camera.ratio, FOCUS_RATIO) },
				{ duration: FOCUS_MS },
			);
		}
	}

	// A patch can remove the focused node, add matches, or change who a focused
	// node's neighbours are, so both views are rebuilt from the graph as it now is.
	private refreshSearch() {
		if (this.focused !== null) {
			if (this.graph.hasNode(this.focused) && !this.hiddenInServiceView(this.focused)) {
				this.focusSet = this.neighbourhood(this.focused);
			} else {
				this.focused = null;
				this.focusSet = null;
				this.options.onFocusChange(null, null);
			}
		}
		this.setQuery(this.searchResult.query);
	}

	private renderLegend() {
		if (this.serviceOnly !== null) this.serviceOnly = serviceEndpoints(this.graph);
		this.options.onServiceChange(serviceSummary(this.graph));

		const counts = new Map<Provider, number>();
		this.graph.forEachNode((_key, attrs) => {
			const provider = providerOf(attrs.kind as string);
			counts.set(provider, (counts.get(provider) ?? 0) + 1);
		});

		const legendData: LegendCount[] = [...counts.entries()]
			.sort((a, b) => b[1] - a[1])
			.map(([provider, count]) => ({
				provider,
				color: PROVIDER_COLORS[provider],
				count,
			}));

		this.options.onLegendChange(legendData);
	}

	private pinBBox(positions: Float32Array) {
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
		this.renderer.setCustomBBox({
			x: [minX - padX, maxX + padX],
			y: [minY - padY, maxY + padY],
		});
		this.renderer.refresh();
	}

	private currentPositions(): { x: number[]; y: number[] } {
		const x: number[] = [];
		const y: number[] = [];
		this.graph.forEachNode((_k, a) => {
			x.push(a.x as number);
			y.push(a.y as number);
		});
		return { x, y };
	}

	private reveal(start: { x: number[]; y: number[] }, target: Float32Array) {
		if (this.anim) cancelAnimationFrame(this.anim);
		const t0 = performance.now();

		const tick = () => {
			const e = easeOutCubic(Math.min(1, (performance.now() - t0) / REVEAL_MS));
			let i = 0;
			this.graph.updateEachNodeAttributes(
				(_k, a) => {
					a.x = start.x[i] + (target[2 * i] - start.x[i]) * e;
					a.y = start.y[i] + (target[2 * i + 1] - start.y[i]) * e;
					i += 1;
					return a;
				},
				{ attributes: ["x", "y"] },
			);
			if (e < 1) {
				this.anim = requestAnimationFrame(tick);
			} else {
				this.phase = "settled";
				this.anim = 0;
			}
			this.updateStatus();
		};
		this.anim = requestAnimationFrame(tick);
	}

	public layoutAndReveal(warm: boolean) {
		if (this.anim) cancelAnimationFrame(this.anim);
		this.engine?.free();
		this.engine = new LayoutEngine(JSON.stringify(snapshotFromGraph(this.graph, warm)));
		if (!warm) this.iterations = 0;

		const start = this.currentPositions();
		this.phase = "laying out";
		let settleIters = 0;

		const settle = () => {
			const budgetStart = performance.now();
			while (
				performance.now() - budgetStart < SETTLE_BUDGET_MS &&
				this.engine!.speed() >= SETTLED_SPEED &&
				settleIters < SETTLE_MAX_ITERS
			) {
				this.engine!.step(SETTLE_STEP);
				settleIters += SETTLE_STEP;
				this.iterations += SETTLE_STEP;
			}
			this.updateStatus();
			if (this.engine!.speed() >= SETTLED_SPEED && settleIters < SETTLE_MAX_ITERS) {
				this.anim = requestAnimationFrame(settle);
				return;
			}
			const target = this.engine!.positionsCopy();
			if (!warm) {
				this.pinBBox(target);
				this.laidOut = true;
				this.renderer.getCamera().animatedReset();
			}
			this.reveal(start, target);
		};
		this.anim = requestAnimationFrame(settle);
	}

	private loadSnapshot(snapshot: Snapshot) {
		try {
			checkVersion(snapshot.version);
			this.graph.clear();
			this.graph.import(buildGraph(snapshot).export());
			this.laidOut = false;
			this.layoutAndReveal(false);
			this.renderLegend();
			this.refreshSearch();
		} catch (e) {
			this.options.onError(errorText(e));
		}
	}

	private onPatch(patch: GraphPatch) {
		try {
			checkVersion(patch.version);
			applyPatch(this.graph, patch);
			if (changesTopology(patch)) this.layoutAndReveal(this.laidOut);
			this.renderLegend();
			if (changesTopology(patch)) this.refreshSearch();
		} catch (e) {
			this.options.onError(errorText(e));
		}
	}

	private requestNeighbors(key: string) {
		if (this.socket?.readyState === WebSocket.OPEN) {
			this.socket.send(JSON.stringify({ type: "get_neighbors", key }));
		}
	}

	private connectLive() {
		const url = serverUrl();
		let gotMessage = false;

		this.socket = new WebSocket(url);

		this.socket.addEventListener("open", () => {
			this.connection = `live: ${url}`;
			this.updateStatus();
			this.socket?.send(JSON.stringify({ type: "subscribe" }));
		});

		this.socket.addEventListener("message", (ev) => {
			gotMessage = true;
			const msg = JSON.parse(ev.data);
			switch (msg.type) {
				case "snapshot":
					this.loadSnapshot(msg as Snapshot);
					break;
				case "patch":
					this.onPatch(msg as GraphPatch);
					break;
				case "collection":
					this.options.onHealthChange(msg as CollectionHealth);
					break;
				case "neighbors":
					console.info(`neighbors of ${msg.key}:`, msg.nodes, msg.edges);
					break;
				case "error":
					console.warn("server error:", msg.message);
					break;
			}
		});

		this.socket.addEventListener("close", () => {
			this.options.onHealthChange(null);
			this.connection = gotMessage ? "disconnected" : "offline";
			this.updateStatus();
			if (!gotMessage) this.fetchStatic();
		});

		this.socket.addEventListener("error", () => {});
	}

	private async fetchStatic() {
		try {
			const res = await fetch("/snapshot.json");
			if (!res.ok) {
				throw new Error(
					`no live server and GET /snapshot.json failed (${res.status}) — start atlas-server or generate a static snapshot.`,
				);
			}
			this.connection = "static";
			this.updateStatus();
			this.loadSnapshot((await res.json()) as Snapshot);
		} catch (e) {
			this.options.onError(errorText(e));
		}
	}

	public destroy() {
		if (this.anim) cancelAnimationFrame(this.anim);
		this.traffic?.destroy();
		this.engine?.free();
		this.renderer.kill();
		this.socket?.close();
	}
}
