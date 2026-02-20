import { test, expect } from '@playwright/test';
import { runSetupWizard, ADMIN, login, createLibraryViaBrowse, triggerScan } from './helpers';

test('@libraries create library, scan, and show items (mp4 discovery)', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  const libName = 'Fixture Movies';
  await createLibraryViaBrowse(page, libName);
  await triggerScan(page, libName);

  await page.goto('/libraries');
  await page.waitForLoadState('networkidle');

  const firstLib = page.locator('a[href^="/libraries/"]').first();
  await expect(firstLib).toBeVisible({ timeout: 30_000 });
  const href = await firstLib.getAttribute('href');
  expect(href).toBeTruthy();

  await page.goto(href!);

  await expect
    .poll(async () => {
      await page.reload();
      await page.waitForLoadState('networkidle');
      const empty = await page.getByText('No media items were found in this library yet.').count();
      return empty === 0;
    }, { timeout: 60_000 })
    .toBe(true);

  await expect(page.locator('a[href^="/items/"]').first()).toBeVisible();
});
