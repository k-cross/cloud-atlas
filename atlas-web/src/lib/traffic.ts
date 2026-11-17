import type Graph from "graphology";
import { flowColor, packetColor } from "./style";

export const FLOW_KIND = "TrafficFlow";

export const FRESHNESS_FADE_MS = 15 * 60 * 1000;

export const MAX_PACKETS_PER_EDGE = 9;

export const MIN_PACKETS_PER_EDGE = 3;

const DOTS_PER_DECADE = 0.9;

const MIN_TRANSIT_MS = 600;
const MAX_TRANSIT_MS = 2200;

export interface ObservedFlow {
	key: string;
	source: string;
	target: string;
	status: string;
	color: string;
	beadColor: string;
	packets: number;
	bytes: number;
	lastSeen: number;
}

export interface TrafficSummary {
	flows: number;
	packets: number;
	bytes: number;
	byStatus: Record<string, number>;
	liveNodes: number;
	newest: number;
}

export function observedFlows(graph: Graph): ObservedFlow[] {
	const flows: ObservedFlow[] = [];
	graph.forEachEdge((key, attrs, source, target) => {
		if (attrs.kind !== FLOW_KIND || attrs.lastSeen === undefined) return;
		const status = (attrs.flowStatus as string) ?? "observed";
		flows.push({
			key,
			source,
			target,
			status,
			color: flowColor(status),
			beadColor: packetColor(status),
			packets: (attrs.packets as number) ?? 0,
			bytes: (attrs.bytes as number) ?? 0,
			lastSeen: attrs.lastSeen as number,
		});
	});
	return flows.sort((a, b) => b.packets - a.packets || a.key.localeCompare(b.key));
}

export function trafficSummary(graph: Graph, flows = observedFlows(graph)): TrafficSummary {
	const byStatus: Record<string, number> = {};
	let packets = 0;
	let bytes = 0;
	let newest = 0;
	for (const flow of flows) {
		byStatus[flow.status] = (byStatus[flow.status] ?? 0) + 1;
		packets += flow.packets;
		bytes += flow.bytes;
		newest = Math.max(newest, flow.lastSeen);
	}

	let liveNodes = 0;
	graph.forEachNode((_key, attrs) => {
		if (attrs.lastSeen === undefined) return;
		liveNodes += 1;
		newest = Math.max(newest, attrs.lastSeen as number);
	});

	return { flows: flows.length, packets, bytes, byStatus, liveNodes, newest };
}

export function freshness(lastSeen: number, now: number, fadeMs = FRESHNESS_FADE_MS): number {
	if (!Number.isFinite(lastSeen) || fadeMs <= 0) return 0;
	const age = now - lastSeen;
	if (age <= 0) return 1;
	if (age >= fadeMs) return 0;
	return 1 - age / fadeMs;
}

export function packetCount(packets: number): number {
	if (!(packets > 0)) return MIN_PACKETS_PER_EDGE;
	const dots = MIN_PACKETS_PER_EDGE + Math.round(Math.log10(packets + 1) * DOTS_PER_DECADE);
	return Math.min(MAX_PACKETS_PER_EDGE, dots);
}

export function transitMs(packets: number): number {
	const busyness = Math.min(1, Math.log10((packets ?? 0) + 1) / 6);
	return MAX_TRANSIT_MS - (MAX_TRANSIT_MS - MIN_TRANSIT_MS) * busyness;
}

export function keyPhase(key: string): number {
	let hash = 0;
	for (let i = 0; i < key.length; i++) {
		hash = (hash * 31 + key.charCodeAt(i)) % 100003;
	}
	return hash / 100003;
}

export function packetPositions(flow: ObservedFlow, nowMs: number): number[] {
	const count = packetCount(flow.packets);
	const base = (nowMs / transitMs(flow.packets) + keyPhase(flow.key)) % 1;
	const positions: number[] = [];
	for (let i = 0; i < count; i++) {
		positions.push((base + i / count) % 1);
	}
	return positions;
}

export function formatBytes(bytes: number): string {
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

export function formatAge(ms: number): string {
	if (!Number.isFinite(ms) || ms < 0) return "—";
	const seconds = Math.floor(ms / 1000);
	if (seconds < 60) return `${seconds}s ago`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m ago`;
	return `${Math.floor(minutes / 60)}h ago`;
}
