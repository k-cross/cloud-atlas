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
    // survive wasm memory reuse. It must run again and settle a second time.
    await page.locator("#reheat").click();
    await expect(page.locator(statusLocator)).toContainText("layout running", {
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
});
