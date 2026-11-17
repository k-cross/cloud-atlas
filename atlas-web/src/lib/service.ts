import type Graph from "graphology";

export const SERVES_KIND = "Serves";

export const CONFIRMED = "confirmed";

export const INFERRED = "inferred";

export interface ServiceSummary {
	confirmed: number;
	inferred: number;
}

export function serviceSummary(graph: Graph): ServiceSummary {
	const summary: ServiceSummary = { confirmed: 0, inferred: 0 };
	graph.forEachEdge((_key, attrs) => {
		if (attrs.kind !== SERVES_KIND) return;
		if (attrs.flowStatus === CONFIRMED) summary.confirmed += 1;
		else summary.inferred += 1;
	});
	return summary;
}

// The nodes a service view keeps. Everything else is the address-level plumbing
// the derived edge exists to summarise, and leaving it drawn underneath is what
// makes the path unreadable.
export function serviceEndpoints(graph: Graph): Set<string> {
	const keys = new Set<string>();
	graph.forEachEdge((_key, attrs, source, target) => {
		if (attrs.kind !== SERVES_KIND) return;
		keys.add(source);
		keys.add(target);
	});
	return keys;
}
