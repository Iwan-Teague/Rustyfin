import { test, expect } from '@playwright/test';
import { runSetupWizard, ADMIN, login } from './helpers';

test('@setup setup completes and admin can login', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  await expect(page).toHaveURL(/\/libraries/);
});
