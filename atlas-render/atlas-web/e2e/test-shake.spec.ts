import { test, expect } from '@playwright/test';

test.use({ video: 'on' });

test('capture video', async ({ page }) => {
  await page.goto('http://localhost:4680/?static');
  await page.waitForTimeout(2000);
  await page.mouse.move(200, 200);
  await page.mouse.down();
  await page.mouse.move(300, 300, { steps: 20 });
  await page.mouse.up();
  await page.waitForTimeout(1000);
});
