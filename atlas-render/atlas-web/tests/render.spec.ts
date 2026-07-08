import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";

// Counts come from the same fixture global-setup serves at /snapshot.json, so
// assertions track the fixture instead of hard-coding numbers. Read lazily:
// the fixture is written by global-setup, which runs after test discovery.
function fixtureCounts(): { nodes: number; edges: number } {
	const snap = JSON.parse(
		readFileSync(
			resolve(dirname(fileURLToPath(import.meta.url)), "../static/snapshot.json"),
			"utf8",
		),
	) as { nodes: unknown[]; edges: unknown[] };
	return { nodes: snap.nodes.length, edges: snap.edges.length };
}

const STATUS = ".status";
const ERROR = ".error-overlay";
const CANVAS = ".graph-container canvas";
// `?static` loads the fixture over one fetch (no server), so render assertions
// are deterministic and independent of the churning demo server.
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

	test("shows the error overlay on a snapshot version mismatch", async ({ page }) => {
		// Guards the SNAPSHOT_VERSION check — the contract that catches an
		// atlas-lib export drift the frontend can't read.
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
		// Regression: Sigma throws "Container has no width" if built into a
		// 0-sized element (a background tab). The app must not crash — it builds
		// tolerantly (allowInvalidContainer) and renders once the container gains
		// size (Sigma's own resize refresh), with no error overlay.
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
		// No "Container has no width" crash while collapsed.
		await expect(page.locator(ERROR)).toHaveCount(0);
		expect(errors).toEqual([]);

		// Laying the container out (tab shown) renders and settles cleanly.
		await page.evaluate(() => document.getElementById("force-zero")?.remove());
		await expect(page.locator(CANVAS).first()).toBeVisible({ timeout: 15_000 });
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
		expect(errors).toEqual([]);
	});

	test("settled graph is rock-still — no per-frame render churn", async ({ page }) => {
		// The core "shaking" regression. Once settled, with a static camera, the
		// rendered image must not change frame-to-frame.
		await page.goto(STATIC);
		await expect(page.locator(STATUS)).toContainText("settled", { timeout: 30_000 });
		await page.waitForTimeout(300);

		const worstDiff: number = await page.evaluate(
			() =>
				new Promise<number>((resolve) => {
					const host = document.querySelector(".graph-container") as HTMLElement;
					const canvases = Array.from(host.querySelectorAll("canvas")) as HTMLCanvasElement[];
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
		// Regression for the "cycling iterations" shake: a live patch pins every
		// pre-existing node and lays out only newcomers, so persistent nodes must
		// not move (beyond f32 round-tripping noise).
		await page.goto("/");
		await page.waitForFunction(
			() => ((globalThis as AtlasHandle).atlas?.graph?.order ?? 0) > 0,
			undefined,
			{ timeout: 15_000 },
		);
		await page.waitForTimeout(3500); // let the initial layout settle

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
