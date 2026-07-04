import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "@playwright/test";

// The snapshot the webServer serves (see playwright.config.ts). Reading it here
// lets the assertions track the fixture instead of hard-coding counts.
const snapshot = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, "../../../multi_cloud_demo.json"),
    "utf8",
  ),
) as { version: number; nodes: unknown[]; edges: unknown[] };

const statusLocator = "#status";
const errorLocator = "#error";

test.describe("cloud-atlas render pipeline", () => {
  test("loads the snapshot and reports node/edge counts", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    await page.goto("/");

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
    await page.goto("/");
    await expect(page.locator(statusLocator)).toContainText("nodes ·", {
      timeout: 15_000,
    });
    // Sigma mounts its WebGL/2D canvases into #graph.
    await expect(page.locator("#graph canvas").first()).toBeVisible();
    const canvasCount = await page.locator("#graph canvas").count();
    expect(canvasCount).toBeGreaterThan(0);
  });

  test("renders a provider legend with colored swatches", async ({ page }) => {
    await page.goto("/");
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
    await page.goto("/");
    // The frame loop stops once engine.speed() drops below SETTLED_SPEED and
    // flips the status to "settled". Generous timeout for slow CI.
    await expect(page.locator(statusLocator)).toContainText("settled", {
      timeout: 30_000,
    });
  });

  test("reheat restarts the layout without crashing", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));

    await page.goto("/");
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

    await page.goto("/");
    const overlay = page.locator(errorLocator);
    await expect(overlay).toBeVisible({ timeout: 15_000 });
    await expect(overlay).toContainText("version 999");
  });
});
