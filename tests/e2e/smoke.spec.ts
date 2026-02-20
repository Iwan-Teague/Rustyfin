import { test, expect } from '@playwright/test';
import { runSetupWizard } from './helpers';

test('@smoke UI loads and redirects to setup if needed', async ({ page }) => {
  await page.goto('/');
  await page.waitForLoadState('domcontentloaded');
  expect(page.url()).toMatch(/\/(setup)?$/);
});

test('@smoke setup wizard completes (if needed)', async ({ page }) => {
  await runSetupWizard(page);
  await expect
    .poll(async () => {
      const doneVisible = await page.getByText('Setup Complete').count();
      return doneVisible > 0 || !page.url().includes('/setup');
    }, { timeout: 20_000 })
    .toBe(true);
});
