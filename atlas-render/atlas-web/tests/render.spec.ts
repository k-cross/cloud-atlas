import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page, test, type WebSocketRoute } from "@playwright/test";

function fixture(): {
	nodes: unknown[];
	edges: unknown[];
	observations?: { packets?: number }[];
} {
	return JSON.parse(
		readFileSync(
			resolve(dirname(fileURLToPath(import.meta.url)), "../static/snapshot.json"),
			"utf8",
		),
	);
}

function fixtureCounts(): { nodes: number; edges: number } {
	const snap = fixture();
	return { nodes: snap.nodes.length, edges: snap.edges.length };
}

function fixtureFlows(): number {
	return (fixture().observations ?? []).filter((o) => o.packets !== undefined).length;
}

function servesKeys(): string[] {
	return (fixture().edges as { key: string; kind: string }[])
		.filter((e) => e.kind === "Serves")
		.map((e) => e.key);
}

function fixtureServes(): number {
	return servesKeys().length;
}

function visibleEdges(page: Page): Promise<number> {
	return page.evaluate(() => {
		const atlas = (
			window as unknown as {
				atlas: {
					graph: { edges: () => string[] };
					renderer: {
						getEdgeDisplayData: (key: string) => { hidden?: boolean } | undefined;
					};
				};
			}
		).atlas;
		return atlas.graph
			.edges()
			.filter((key) => atlas.renderer.getEdgeDisplayData(key)?.hidden !== true).length;
	});
}

const STATUS = ".status";
const ERROR = ".error-overlay";
const CANVAS = ".graph-container canvas";

const STATIC = "/?static=1";

async function nodeCount(page: Page): Promise<number> {
	const text = (await page.locator(STATUS).textContent()) ?? "";
	const match = text.match(/(\d+) nodes/);
	return match ? Number(match[1]) : Number.NaN;
}

interface AtlasHandle {
	atlas?: { graph?: { order: number } };
}

test.describe("render pipeline (static)", () => {
	test("loads the snapshot and reports node/edge counts", async ({ page }) => {
		const errors: string[] = [];
		page.on("pageerror", (e) => errors.push(e.message));

		await page.goto(STATIC);

		const { nodes, edges } = fixtureCounts();
		await expect(page.locator(STATUS)).toContainText(`${nodes} nodes · ${edges} edges`, {
			timeout: 15_000,
		});
		await expect(page.locator(ERROR)).toHaveCount(0);
		expect(errors).toEqual([]);
	});

	test("renders a Sigma WebGL canvas", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("nodes ·", { timeout: 15_000 });
		await expect(page.locator(CANVAS).first()).toBeVisible();
		expect(await page.locator(CANVAS).count()).toBeGreaterThan(0);
	});

	test("renders a provider legend with colored swatches", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(".legend-row").first()).toBeVisible({ timeout: 15_000 });

		const swatch = page.locator(".swatch").first();
		const bg = await swatch.evaluate((el) => getComputedStyle(el).backgroundColor);
		expect(bg).not.toBe("rgba(0, 0, 0, 0)");

		const counts = await page.locator(".count").allTextContents();
		expect(counts.length).toBeGreaterThan(0);
		for (const c of counts) expect(Number(c)).toBeGreaterThan(0);
	});

	test("the layout settles (physics converges and stops)", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
	});

	test("reheat re-lays out the graph and settles again", async ({ page }) => {
		const errors: string[] = [];
		page.on("pageerror", (e) => errors.push(e.message));

		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });

		await page.getByRole("button", { name: /reheat/i }).click();
		await expect(page.locator(STATUS)).toContainText("laying out", { timeout: 5_000 });
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });

		await expect(page.locator(ERROR)).toHaveCount(0);
		expect(errors).toEqual([]);
	});

	test("shows the observed-traffic panel with the fixture's flows", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(".traffic")).toBeVisible({ timeout: 15_000 });
		await expect(page.locator(".traffic")).toContainText(`${fixtureFlows()} flows`);
		await expect(page.locator(".chip").first()).toBeVisible();
	});

	test("traffic animates on its own canvas while the layout stays still", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
		await page.waitForTimeout(300);

		const moved: number = await page.evaluate(
			() =>
				new Promise<number>((resolve) => {
					const canvas = document.querySelector(
						".graph-container canvas.traffic-layer",
					) as HTMLCanvasElement;
					const W = 300;
					const H = 220;
					const off = document.createElement("canvas");
					off.width = W;
					off.height = H;
					const ctx = off.getContext("2d")!;
					const grab = (): Uint8ClampedArray => {
						ctx.clearRect(0, 0, W, H);
						ctx.drawImage(canvas, 0, 0, W, H);
						return ctx.getImageData(0, 0, W, H).data;
					};
					const first = grab();
					setTimeout(() => {
						const later = grab();
						let sum = 0;
						for (let i = 0; i < later.length; i += 4) sum += Math.abs(later[i] - first[i]);
						resolve(sum);
					}, 700);
				}),
		);
		expect(moved).toBeGreaterThan(0);
	});

	test("shows the derived service topology with both provenances", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(".service")).toBeVisible({ timeout: 15_000 });
		await expect(page.locator(".service")).toContainText("confirmed");
		await expect(page.locator(".service")).toContainText("inferred");
	});

	test("service view hides the plumbing the derived edges summarise", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });

		const before = await visibleEdges(page);
		await page.locator(".service input[type=checkbox]").check();
		await expect.poll(() => visibleEdges(page)).toBe(fixtureServes());

		expect(before).toBeGreaterThan(fixtureServes());

		await page.locator(".service input[type=checkbox]").uncheck();
		await expect.poll(() => visibleEdges(page)).toBe(before);
	});

	test("shows the error overlay on a snapshot version mismatch", async ({ page }) => {
		await page.route("**/snapshot.json", (route) =>
			route.fulfill({
				contentType: "application/json",
				body: JSON.stringify({ version: 999, nodes: [], edges: [] }),
			}),
		);
		await page.goto(STATIC);
		await expect(page.locator(ERROR)).toBeVisible({ timeout: 15_000 });
		await expect(page.locator(ERROR)).toContainText("version 999");
	});

	test("survives a zero-width container, then renders once laid out", async ({ page }) => {
		const errors: string[] = [];
		page.on("pageerror", (e) => errors.push(e.message));

		await page.addInitScript(() => {
			const inject = () => {
				const s = document.createElement("style");
				s.id = "force-zero";
				s.textContent =
					".graph-container{width:0!important;height:0!important;inset:auto!important}";
				(document.head || document.documentElement).appendChild(s);
			};
			if (document.documentElement) inject();
			else
				new MutationObserver((_m, o) => {
					if (document.documentElement) {
						o.disconnect();
						inject();
					}
				}).observe(document, { childList: true, subtree: true });
		});

		await page.goto(STATIC);
		await page.waitForTimeout(1000);

		await expect(page.locator(ERROR)).toHaveCount(0);
		expect(errors).toEqual([]);

		await page.evaluate(() => document.getElementById("force-zero")?.remove());
		await expect(page.locator(CANVAS).first()).toBeVisible({ timeout: 15_000 });
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
		expect(errors).toEqual([]);
	});

	test("settled graph is rock-still — no per-frame render churn", async ({ page }) => {
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
		await page.waitForTimeout(300);

		const worstDiff: number = await page.evaluate(
			() =>
				new Promise<number>((resolve) => {
					const host = document.querySelector(".graph-container") as HTMLElement;
					const canvases = Array.from(
						host.querySelectorAll("canvas:not(.traffic-layer)"),
					) as HTMLCanvasElement[];
					const W = 300;
					const H = 220;
					const off = document.createElement("canvas");
					off.width = W;
					off.height = H;
					const ctx = off.getContext("2d")!;
					const grab = (): Uint8ClampedArray => {
						ctx.clearRect(0, 0, W, H);
						for (const c of canvases) ctx.drawImage(c, 0, 0, W, H);
						return ctx.getImageData(0, 0, W, H).data;
					};
					let prev = grab();
					let worst = 0;
					let f = 0;
					const tick = () => {
						const cur = grab();
						let sum = 0;
						for (let i = 0; i < cur.length; i += 4) sum += Math.abs(cur[i] - prev[i]);
						worst = Math.max(worst, sum / (W * H));
						prev = cur;
						if (++f < 20) requestAnimationFrame(tick);
						else resolve(worst);
					};
					requestAnimationFrame(tick);
				}),
		);
		expect(worstDiff).toBeLessThan(0.5);
	});
});

test.describe("service view (mocked backend)", () => {
	const MOCK = "ws://mock.invalid/ws";

	// The toggle used to live inside the "anything derived?" guard, so a patch
	// that removed the last Serves edge hid every node *and* the only control
	// that could bring them back.
	test("stays reachable after the last derived edge is removed", async ({ page }) => {
		const snapshot = fixture() as unknown as Record<string, unknown> & { version: number };
		let server: WebSocketRoute | undefined;
		await page.routeWebSocket(MOCK, (ws) => {
			server = ws;
			ws.onMessage((message) => {
				if (JSON.parse(String(message)).type === "subscribe") {
					ws.send(JSON.stringify({ ...snapshot, type: "snapshot" }));
				}
			});
		});

		await page.goto(`/?server=${MOCK}`);
		const toggle = page.locator(".service input[type=checkbox]");
		await toggle.check({ timeout: 15_000 });
		await expect.poll(() => visibleEdges(page)).toBe(fixtureServes());

		server?.send(
			JSON.stringify({
				type: "patch",
				version: snapshot.version,
				added_nodes: [],
				removed_nodes: [],
				added_edges: [],
				removed_edges: servesKeys(),
			}),
		);

		await expect(page.locator(".service")).toContainText("nothing derived yet");
		await expect(toggle).toBeVisible();

		// Turning the view off with nothing derived takes the panel away again,
		// so the checkbox is gone before its state could be read back.
		await toggle.click();
		await expect(page.locator(".service")).toHaveCount(0);
		await expect.poll(() => visibleEdges(page)).toBeGreaterThan(0);
	});
});

test.describe("live backend (WebSocket)", () => {
	test("connects to atlas-server and renders the pushed snapshot", async ({ page }) => {
		const errors: string[] = [];
		page.on("pageerror", (e) => errors.push(e.message));

		await page.goto("/");
		await expect(page.locator(STATUS)).toContainText("live:", { timeout: 15_000 });
		await expect(page.locator(STATUS)).toContainText("nodes ·", { timeout: 15_000 });
		await expect(page.locator(ERROR)).toHaveCount(0);
		expect(errors).toEqual([]);
	});

	test("applies live patches as the demo graph churns", async ({ page }) => {
		await page.goto("/");
		await expect(page.locator(STATUS)).toContainText("nodes ·", { timeout: 15_000 });

		const initial = await nodeCount(page);
		expect(Number.isNaN(initial)).toBe(false);
		await expect.poll(() => nodeCount(page), { timeout: 20_000 }).not.toBe(initial);
	});

	test("patches pin existing nodes — the cloud does not move", async ({ page }) => {
		await page.goto("/");
		await page.waitForFunction(
			() => ((globalThis as AtlasHandle).atlas?.graph?.order ?? 0) > 0,
			undefined,
			{ timeout: 15_000 },
		);
		await page.waitForTimeout(3500);

		const maxMove: number = await page.evaluate(
			() =>
				new Promise<number>((resolve) => {
					// biome-ignore lint/suspicious/noExplicitAny: test-only introspection handle
					const graph = (globalThis as any).atlas.graph;
					const posOf = (): Record<string, [number, number]> => {
						const m: Record<string, [number, number]> = {};
						graph.forEachNode((k: string, a: { x: number; y: number }) => {
							m[k] = [a.x, a.y];
						});
						return m;
					};
					const before = posOf();
					const startOrder = graph.order;
					const t0 = Date.now();
					const wait = () => {
						if (graph.order !== startOrder) {
							setTimeout(() => {
								const after = posOf();
								let max = 0;
								for (const k of Object.keys(before)) {
									if (after[k]) {
										max = Math.max(
											max,
											Math.hypot(after[k][0] - before[k][0], after[k][1] - before[k][1]),
										);
									}
								}
								resolve(max);
							}, 1500);
						} else if (Date.now() - t0 > 12_000) resolve(Number.POSITIVE_INFINITY);
						else setTimeout(wait, 100);
					};
					wait();
				}),
		);
		expect(maxMove).toBeLessThan(0.5);
	});
});
