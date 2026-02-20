import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { runSetupWizard, ADMIN, login } from './helpers';

async function a11yPage(page, path: string) {
  await page.goto(path);
  await page.waitForLoadState('networkidle');
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations, `Accessibility violations on ${path}`).toEqual([]);
}

test('@a11y basic pages pass axe checks (this will surface real UI issues)', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  await a11yPage(page, '/libraries');
  await a11yPage(page, '/admin');

  await expect(true).toBeTruthy();
});
