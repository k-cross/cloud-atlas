import type Graph from "graphology";

export interface SearchHit {
	key: string;
	label: string;
	kind: string;
}

export interface SearchResult {
	query: string;
	hits: SearchHit[];
	matches: Set<string>;
	total: number;
}

export interface Segment {
	text: string;
	match: boolean;
}

export const SEARCH_LIMIT = 8;

export function emptySearch(query = ""): SearchResult {
	return { query, hits: [], matches: new Set(), total: 0 };
}

export function searchTokens(query: string): string[] {
	return query.toLowerCase().split(/\s+/).filter(Boolean);
}

const WORD = /[a-z0-9]/;

function tokenScore(label: string, kind: string, token: string): number | null {
	let best: number | null = null;
	for (let at = label.indexOf(token); at >= 0; at = label.indexOf(token, at + 1)) {
		const end = at + token.length;
		const starts = at === 0 || !WORD.test(label[at - 1]);
		const ends = end === label.length || !WORD.test(label[end]);
		const score = starts && ends ? 0 : starts ? 1 : 2;
		best = best === null ? score : Math.min(best, score);
	}
	if (best !== null) return best;
	return kind.includes(token) ? 3 : null;
}

function rank(label: string, kind: string, tokens: string[]): number | null {
	const lowerLabel = label.toLowerCase();
	const lowerKind = kind.toLowerCase();
	let score = 0;
	for (const token of tokens) {
		const tokenRank = tokenScore(lowerLabel, lowerKind, token);
		if (tokenRank === null) return null;
		score += tokenRank;
	}
	return score;
}

export function searchNodes(
	graph: Graph,
	query: string,
	include: (key: string) => boolean = () => true,
	limit = SEARCH_LIMIT,
): SearchResult {
	const tokens = searchTokens(query);
	if (tokens.length === 0) return emptySearch(query);

	const matches = new Set<string>();
	const ranked: { hit: SearchHit; score: number }[] = [];
	graph.forEachNode((key, attrs) => {
		if (!include(key)) return;
		const label = String(attrs.label ?? key);
		const kind = String(attrs.kind ?? "");
		const score = rank(label, kind, tokens);
		if (score === null) return;
		matches.add(key);
		ranked.push({ hit: { key, label, kind }, score });
	});

	ranked.sort(
		(a, b) =>
			a.score - b.score ||
			a.hit.label.length - b.hit.label.length ||
			a.hit.label.localeCompare(b.hit.label),
	);
	return {
		query,
		hits: ranked.slice(0, limit).map((r) => r.hit),
		matches,
		total: ranked.length,
	};
}

function matchedRanges(label: string, tokens: string[]): [number, number][] {
	const lower = label.toLowerCase();
	const ranges: [number, number][] = [];
	for (const token of tokens) {
		for (let at = lower.indexOf(token); at >= 0; at = lower.indexOf(token, at + 1)) {
			ranges.push([at, at + token.length]);
		}
	}
	ranges.sort((a, b) => a[0] - b[0]);

	const merged: [number, number][] = [];
	for (const [start, end] of ranges) {
		const last = merged[merged.length - 1];
		if (last && start <= last[1]) last[1] = Math.max(last[1], end);
		else merged.push([start, end]);
	}
	return merged;
}

// Labels run to 150-odd characters and the part that names the resource is
// usually at the end, so a long label is cut to a window around its first match
// (or to its tail when only the kind matched) instead of truncated from the end.
export function excerpt(label: string, tokens: string[], width = 38): Segment[] {
	const ranges = matchedRanges(label, tokens);
	let start = 0;
	let end = label.length;
	if (label.length > width) {
		start =
			ranges.length > 0
				? Math.max(0, Math.min(ranges[0][0] - Math.floor(width / 3), label.length - width))
				: label.length - width;
		end = start + width;
	}

	const segments: Segment[] = [];
	const push = (text: string, match: boolean) => {
		if (text) segments.push({ text, match });
	};
	if (start > 0) push("…", false);
	let cursor = start;
	for (const [from, to] of ranges) {
		const clippedFrom = Math.max(from, start);
		const clippedTo = Math.min(to, end);
		if (clippedFrom >= clippedTo) continue;
		push(label.slice(cursor, clippedFrom), false);
		push(label.slice(clippedFrom, clippedTo), true);
		cursor = clippedTo;
	}
	push(label.slice(cursor, end), false);
	if (end < label.length) push("…", false);
	return segments;
}
