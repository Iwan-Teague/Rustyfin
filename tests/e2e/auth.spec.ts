import { test, expect } from '@playwright/test';
import { runSetupWizard, ADMIN, login } from './helpers';

test('@auth navbar should not show Login once authenticated (regression)', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  // Desired behavior: when logged in, navbar shows Logout and does not show Login.
  await expect(page.getByRole('link', { name: 'Login' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Logout' })).toBeVisible();
});

test('@auth should support logout (regression)', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  await page.getByRole('button', { name: 'Logout' }).click();
  await expect(page).toHaveURL(/\/login/);
});
