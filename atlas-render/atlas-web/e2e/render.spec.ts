import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { type Page, expect, test } from "@playwright/test";

// The snapshot serve.ts hosts (see playwright.config.ts). Reading it here lets
// the assertions track the fixture instead of hard-coding counts.
const snapshot = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, "../../../multi_cloud_demo.json"),
    "utf8",
  ),
) as { version: number; nodes: unknown[]; edges: unknown[] };

const statusLocator = "#status";
const errorLocator = "#error";

// `?static` loads the snapshot over one HTTP fetch instead of the live
// WebSocket, so the render-pipeline assertions are deterministic and don't
// depend on the churning demo server.
const STATIC = "/?static=1";

async function nodeCount(page: Page): Promise<number> {
  const text = (await page.locator(statusLocator).textContent()) ?? "";
  const match = text.match(/(\d+) nodes/);
  return match ? Number(match[1]) : Number.NaN;
}

test.describe("cloud-atlas render pipeline (static)", () => {
  test("loads the snapshot and reports node/edge counts", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    await page.goto(STATIC);

    // main() resolves fetch + wasm init + Sigma construction before the status
    // ever leaves "loading…". Wait for the real count line to appear.
    await expect(page.locator(statusLocator)).toContainText(
      `${snapshot.nodes.length} nodes · ${snapshot.edges.length} edges`,
      { timeout: 15_000 },
    );

    // The error overlay must never have been shown.
    await expect(page.locator(errorLocator)).toBeHidden();
    expect(errors).toEqual([]);
  });

  test("renders a Sigma WebGL canvas", async ({ page }) => {
    await page.goto(STATIC);
    await expect(page.locator(statusLocator)).toContainText("nodes ·", {
      timeout: 15_000,
    });
    // Sigma mounts its WebGL/2D canvases into #graph.
    await expect(page.locator("#graph canvas").first()).toBeVisible();
    const canvasCount = await page.locator("#graph canvas").count();
    expect(canvasCount).toBeGreaterThan(0);
  });

  test("renders a provider legend with colored swatches", async ({ page }) => {
    await page.goto(STATIC);
    await expect(page.locator("#legend > div").first()).toBeVisible({
      timeout: 15_000,
    });

    const rows = page.locator("#legend > div");
    expect(await rows.count()).toBeGreaterThan(0);

    // Every legend row carries a colored swatch and a numeric count.
    const swatch = page.locator("#legend .swatch").first();
    const bg = await swatch.evaluate(
      (el) => getComputedStyle(el).backgroundColor,
    );
    expect(bg).not.toBe("rgba(0, 0, 0, 0)");

    const counts = await page.locator("#legend .count").allTextContents();
    expect(counts.length).toBeGreaterThan(0);
    for (const c of counts) expect(Number(c)).toBeGreaterThan(0);
  });

  test("the layout settles (physics converges and stops)", async ({ page }) => {
    await page.goto(STATIC);
    // The frame loop stops once engine.speed() drops below SETTLED_SPEED and
    // flips the status to "settled". Generous timeout for slow CI.
    await expect(page.locator(statusLocator)).toContainText("settled", {
      timeout: 30_000,
    });
  });

  test("reheat restarts the layout without crashing", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    await page.goto(STATIC);
    await expect(page.locator(statusLocator)).toContainText("settled", {
      timeout: 30_000,
    });

    // Reheat frees the wasm engine and builds a fresh one — a path that has to
    // survive wasm memory reuse. It must lay out again and settle a second time.
    await page.locator("#reheat").click();
    await expect(page.locator(statusLocator)).toContainText("laying out", {
      timeout: 5_000,
    });
    await expect(page.locator(statusLocator)).toContainText("settled", {
      timeout: 30_000,
    });

    await expect(page.locator(errorLocator)).toBeHidden();
    expect(errors).toEqual([]);
  });

  test("shows the error overlay on a snapshot version mismatch", async ({
    page,
  }) => {
    // Guards the fail() path and the SNAPSHOT_VERSION check — the contract that
    // catches an atlas-lib export drift the frontend can't read.
    await page.route("**/snapshot.json", (route) =>
      route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({ version: 999, nodes: [], edges: [] }),
      }),
    );

    await page.goto(STATIC);
    const overlay = page.locator(errorLocator);
    await expect(overlay).toBeVisible({ timeout: 15_000 });
    await expect(overlay).toContainText("version 999");
  });

  test("survives a zero-width container until it is laid out", async ({
    page,
  }) => {
    // Regression: Sigma throws "Container has no width" if built into a
    // 0-sized element (a background tab / not-yet-shown window). The app must
    // wait for the container to have size instead of crashing.
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    // Force #graph to zero size before the (deferred) app module runs.
    await page.addInitScript(() => {
      const inject = () => {
        const s = document.createElement("style");
        s.id = "force-zero";
        s.textContent =
          "#graph{width:0!important;height:0!important;inset:auto!important;position:absolute!important}";
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
    // No Sigma crash while zero-sized: it waits, showing no canvas and no error.
    await page.waitForTimeout(1000);
    await expect(page.locator(errorLocator)).toBeHidden();
    expect(await page.locator("#graph canvas").count()).toBe(0);

    // Laying the container out (tab shown) must let Sigma build and render.
    await page.evaluate(() => document.getElementById("force-zero")?.remove());
    await expect(page.locator("#graph canvas").first()).toBeVisible({
      timeout: 15_000,
    });
    await expect(page.locator(statusLocator)).toContainText("nodes ·");
    expect(errors).toEqual([]);
  });

  test("settled graph is rock-still — no per-frame render churn", async ({
    page,
  }) => {
    // The core "shaking" regression. Once settled, with a static camera, the
    // rendered image must not change frame-to-frame: positions are static and
    // the coordinate box is pinned, so Sigma does not re-normalize/re-fit.
    await page.goto(STATIC);
    await expect(page.locator(statusLocator)).toContainText("settled", {
      timeout: 30_000,
    });
    await page.waitForTimeout(300);

    const pixelDiff: number = await page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const host = document.getElementById("graph")!;
          const canvases = Array.from(
            host.querySelectorAll("canvas"),
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

    // Perfectly still is 0; allow a hair for AA/rounding noise.
    expect(pixelDiff).toBeLessThan(0.5);
  });
});

test.describe("cloud-atlas live backend (WebSocket)", () => {
  test("connects to atlas-server and renders the pushed snapshot", async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    await page.goto("/");

    // The status line reports the live connection and a populated graph once
    // the server pushes its first snapshot.
    await expect(page.locator(statusLocator)).toContainText("live:", {
      timeout: 15_000,
    });
    await expect(page.locator(statusLocator)).toContainText("nodes ·", {
      timeout: 15_000,
    });
    await expect(page.locator(errorLocator)).toBeHidden();
    expect(errors).toEqual([]);
  });

  test("applies live patches as the demo graph churns", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(statusLocator)).toContainText("nodes ·", {
      timeout: 15_000,
    });

    // The demo server flips a sentinel node/edge in and out every couple of
    // seconds; the node count must change as those patches are applied.
    const initial = await nodeCount(page);
    expect(Number.isNaN(initial)).toBe(false);
    await expect
      .poll(() => nodeCount(page), { timeout: 20_000 })
      .not.toBe(initial);
  });

  test("patches pin existing nodes — the cloud does not move at all", async ({
    page,
  }) => {
    // Regression for the "cycling iterations" shake: a live patch pins every
    // pre-existing node in the engine and lays out only the newcomers, so
    // persistent nodes must not move (beyond f32 round-tripping noise).
    await page.goto("/");
    await page.waitForFunction(
      () =>
        ((window as unknown as { atlas?: { graph?: { order: number } } }).atlas
          ?.graph?.order ?? 0) > 0,
      undefined,
      { timeout: 15_000 },
    );
    await page.waitForTimeout(3500); // let the initial layout settle

    const maxMove: number = await page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const graph = (window as unknown as { atlas: { graph: any } }).atlas
            .graph;
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
                      Math.hypot(
                        after[k][0] - before[k][0],
                        after[k][1] - before[k][1],
                      ),
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

    // Pinned means pinned: only float32 JSON round-tripping noise is allowed.
    expect(maxMove).toBeLessThan(0.5);
  });
});
