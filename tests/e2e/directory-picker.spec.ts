import { test, expect } from '@playwright/test';
import { runSetupWizard, ADMIN, login } from './helpers';

test('@dirpicker Browse should populate path and not claim macOS unsupported', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  await page.goto('/admin');
  await page.getByRole('button', { name: 'Browse' }).click();

  await expect(page.getByText('Directory selected')).toBeVisible();
  await expect(page.getByText(/only supported on macOS/i)).toHaveCount(0);
  await expect(page.getByPlaceholder('/path/to/media')).not.toHaveValue('');
});
