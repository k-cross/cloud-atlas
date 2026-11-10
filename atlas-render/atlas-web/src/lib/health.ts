export interface CollectionFailure {
	source: string;
	kind: string;
	scope: string;
	message: string;
}

export interface UnreadableSource {
	source: string;
	kind: string;
	scans: number;
	budget: number;
	held: boolean;
}

export interface FeedHealth {
	healthy: boolean;
	failures: CollectionFailure[];
}

export interface CollectionHealth {
	complete: boolean;
	unreadable: string[];
	retention: UnreadableSource[];
	failures: CollectionFailure[];
	stream: FeedHealth;
	flows: FeedHealth & { observed: number };
}

export type HealthLevel = "ok" | "degraded" | "holding" | "released";

export interface HealthRow {
	subject: string;
	detail: string;
	level: HealthLevel;
	failures: string[];
}

const SEVERITY: HealthLevel[] = ["ok", "degraded", "holding", "released"];

const MALFORMED = "malformed";

const SOURCE_NAMES: Record<string, string> = {
	aws: "AWS",
	gcp: "GCP",
	azure: "Azure",
	cloudflare: "Cloudflare",
};

export function sourceName(source: string): string {
	return SOURCE_NAMES[source] ?? source;
}

function plural(count: number, noun: string): string {
	return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function describe(failure: CollectionFailure): string {
	return `${failure.scope}: ${failure.message}`;
}

function unreadableKind(failures: CollectionFailure[]): string {
	const unreadable = failures.filter((f) => f.kind !== MALFORMED);
	return unreadable.every((f) => f.kind === "unauthorized") ? "unauthorized" : "unavailable";
}

function feedRow(subject: string, feed: FeedHealth): HealthRow[] {
	if (feed.healthy) return [];
	return [
		{
			subject,
			detail: plural(feed.failures.length, "failure"),
			level: "degraded",
			failures: feed.failures.map(describe),
		},
	];
}

export function healthRows(health: CollectionHealth): HealthRow[] {
	const failuresOf = (source: string) => health.failures.filter((f) => f.source === source);
	const retained = new Set(health.retention.map((r) => r.source));

	const rows: HealthRow[] = health.retention.map((r) => ({
		subject: sourceName(r.source),
		detail: r.held
			? `${r.kind} · holding ${r.scans}/${r.budget} scans`
			: `${r.kind} · released after ${r.budget} scans`,
		level: r.held ? "holding" : "released",
		failures: failuresOf(r.source).map(describe),
	}));

	const others = [...new Set(health.failures.map((f) => f.source))]
		.filter((source) => !retained.has(source))
		.sort();
	for (const source of others) {
		const failures = failuresOf(source);
		const unreadable = failures.some((f) => f.kind !== MALFORMED);
		rows.push({
			subject: sourceName(source),
			detail: unreadable
				? `${unreadableKind(failures)} · nothing held yet`
				: `${plural(failures.length, "malformed row")} · removals still apply`,
			level: unreadable ? "released" : "degraded",
			failures: failures.map(describe),
		});
	}

	return [...rows, ...feedRow("Event feed", health.stream), ...feedRow("Flow feed", health.flows)];
}

export function healthLevel(rows: HealthRow[]): HealthLevel {
	return rows.reduce<HealthLevel>(
		(worst, row) => (SEVERITY.indexOf(row.level) > SEVERITY.indexOf(worst) ? row.level : worst),
		"ok",
	);
}
