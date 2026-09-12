import type Graph from "graphology";
import type Sigma from "sigma";
import { FLOW_STATUS_COLORS } from "./style";
import {
	freshness,
	type ObservedFlow,
	observedFlows,
	packetPositions,
	type TrafficSummary,
	trafficSummary,
} from "./traffic";

export const TRAFFIC_CANVAS_CLASS = "traffic-layer";

const MIN_ALPHA = 0.3;
const PACKET_RADIUS = 3.4;
const PACKET_GLOW = 10;
const HALO_WIDTH = 2.5;
const HALO_PULSE_MS = 2600;
const REFRESH_MS = 400;

type Point = { x: number; y: number };
type LiveNode = { key: string; lastSeen: number; color: string };

export class TrafficLayer {
	private renderer: Sigma;
	private graph: Graph;
	private canvas: HTMLCanvasElement;
	private ctx: CanvasRenderingContext2D;
	private observer: ResizeObserver;
	private frame = 0;
	private lastRefresh = 0;
	private flows: ObservedFlow[] = [];
	private live: LiveNode[] = [];
	private onSummary: (summary: TrafficSummary) => void;

	constructor(renderer: Sigma, graph: Graph, onSummary: (summary: TrafficSummary) => void) {
		this.renderer = renderer;
		this.graph = graph;
		this.onSummary = onSummary;

		this.canvas = document.createElement("canvas");
		this.canvas.className = TRAFFIC_CANVAS_CLASS;
		this.canvas.style.position = "absolute";
		this.canvas.style.inset = "0";
		this.canvas.style.pointerEvents = "none";
		this.canvas.style.zIndex = "1";
		renderer.getContainer().appendChild(this.canvas);
		this.ctx = this.canvas.getContext("2d")!;

		this.observer = new ResizeObserver(() => this.resize());
		this.observer.observe(renderer.getContainer());
		this.resize();
	}

	public start() {
		if (this.frame) return;
		const tick = () => {
			this.draw(performance.now());
			this.frame = requestAnimationFrame(tick);
		};
		this.frame = requestAnimationFrame(tick);
	}

	public stop() {
		if (this.frame) cancelAnimationFrame(this.frame);
		this.frame = 0;
	}

	public destroy() {
		this.stop();
		this.observer.disconnect();
		this.canvas.remove();
	}

	private resize() {
		const { width, height } = this.renderer.getDimensions();
		const ratio = window.devicePixelRatio || 1;
		this.canvas.width = Math.max(1, Math.floor(width * ratio));
		this.canvas.height = Math.max(1, Math.floor(height * ratio));
		this.canvas.style.width = `${width}px`;
		this.canvas.style.height = `${height}px`;
		this.ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
	}

	private refresh() {
		this.flows = observedFlows(this.graph);
		this.live = [];
		this.graph.forEachNode((key, attrs) => {
			if (attrs.lastSeen === undefined) return;
			this.live.push({
				key,
				lastSeen: attrs.lastSeen as number,
				color: haloColor(attrs.flowStatus as string | undefined),
			});
		});
		this.onSummary(trafficSummary(this.graph, this.flows));
	}

	private viewport(key: string): Point | null {
		const display = this.renderer.getNodeDisplayData(key);
		if (!display || display.hidden) return null;
		return this.renderer.framedGraphToViewport({ x: display.x, y: display.y });
	}

	private draw(now: number) {
		if (now - this.lastRefresh > REFRESH_MS) {
			this.lastRefresh = now;
			this.refresh();
		}

		const { width, height } = this.renderer.getDimensions();
		this.ctx.clearRect(0, 0, width, height);
		if (this.flows.length === 0 && this.live.length === 0) return;

		const wallClock = Date.now();
		this.drawHalos(now, wallClock);
		for (const flow of this.flows) this.drawFlow(flow, now, wallClock);
	}

	private drawFlow(flow: ObservedFlow, now: number, wallClock: number) {
		const from = this.viewport(flow.source);
		const to = this.viewport(flow.target);
		if (!from || !to) return;

		this.ctx.save();
		this.ctx.globalAlpha = alphaFor(flow.lastSeen, wallClock);
		this.ctx.fillStyle = flow.beadColor;
		this.ctx.shadowColor = flow.color;
		this.ctx.shadowBlur = PACKET_GLOW;

		for (const t of packetPositions(flow, now)) {
			this.ctx.beginPath();
			this.ctx.arc(
				from.x + (to.x - from.x) * t,
				from.y + (to.y - from.y) * t,
				PACKET_RADIUS,
				0,
				Math.PI * 2,
			);
			this.ctx.fill();
		}
		this.ctx.restore();
	}

	private drawHalos(now: number, wallClock: number) {
		const pulse = 0.5 + 0.5 * Math.sin((now / HALO_PULSE_MS) * Math.PI * 2);
		this.ctx.save();
		this.ctx.lineWidth = HALO_WIDTH;
		for (const node of this.live) {
			const point = this.viewport(node.key);
			const display = this.renderer.getNodeDisplayData(node.key);
			if (!point || !display) continue;

			this.ctx.globalAlpha = alphaFor(node.lastSeen, wallClock) * (0.35 + 0.4 * pulse);
			this.ctx.strokeStyle = node.color;
			this.ctx.beginPath();
			this.ctx.arc(
				point.x,
				point.y,
				this.renderer.scaleSize(display.size) + 4 + 2 * pulse,
				0,
				Math.PI * 2,
			);
			this.ctx.stroke();
		}
		this.ctx.restore();
	}
}

function alphaFor(lastSeen: number, wallClock: number): number {
	return MIN_ALPHA + (1 - MIN_ALPHA) * freshness(lastSeen, wallClock);
}

function haloColor(status: string | undefined): string {
	return FLOW_STATUS_COLORS[status ?? "observed"] ?? FLOW_STATUS_COLORS.observed;
}
