import { expect, Page } from '@playwright/test';

export const ADMIN = {
  username: 'admin',
  password: 'AdminPassword123!' // >= 12 chars
};

export const USER = {
  username: 'basicuser',
  password: 'UserPassword123!' // >= 12 chars
};

export async function runSetupWizard(page: Page) {
  await page.goto('/setup');
  await page.waitForLoadState('domcontentloaded');
  await page.waitForTimeout(300);

  // If setup is already complete, /setup will redirect away.
  if (!page.url().includes('/setup')) return;

  // Already on done state in the wizard.
  if (await page.getByText('Setup Complete').count()) return;

  await expect(page.getByRole('button', { name: 'Get Started', exact: true })).toBeVisible({ timeout: 15_000 });
  await page.getByRole('button', { name: 'Get Started', exact: true }).click();

  const configSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Server Configuration' }),
  });
  await expect(configSection).toBeVisible({ timeout: 15_000 });
  await configSection.locator('input[type="text"]').first().fill('Rustyfin Test Server');
  await configSection.getByRole('button', { name: 'Next', exact: true }).click();

  const adminSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Create Admin Account' }),
  });
  await expect(adminSection).toBeVisible({ timeout: 15_000 });
  await adminSection.locator('input[type="text"]').first().fill(ADMIN.username);
  await adminSection.locator('input[type="password"]').nth(0).fill(ADMIN.password);
  await adminSection.locator('input[type="password"]').nth(1).fill(ADMIN.password);
  await adminSection.getByRole('button', { name: 'Next', exact: true }).click();

  const metadataSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Metadata Preferences' }),
  });
  await expect(metadataSection).toBeVisible({ timeout: 15_000 });
  await metadataSection.getByRole('button', { name: 'Next', exact: true }).click(); // metadata

  const networkSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Network Settings' }),
  });
  await expect(networkSection).toBeVisible({ timeout: 15_000 });
  await networkSection.getByRole('button', { name: 'Next', exact: true }).click(); // network

  await expect(page.getByRole('heading', { name: 'Ready to Go' })).toBeVisible({ timeout: 15_000 });

  await page.getByRole('button', { name: 'Finish Setup', exact: true }).click();
  await expect(page.getByText('Setup Complete')).toBeVisible({ timeout: 20_000 });
}

export async function login(page: Page, username: string, password: string) {
  await page.goto('/login');
  await page.waitForLoadState('domcontentloaded');

  const form = page.locator('form').first();
  await expect(form).toBeVisible({ timeout: 15_000 });

  await form.locator('input[type="text"]').first().fill(username);
  await form.locator('input[type="password"]').first().fill(password);

  await Promise.all([
    page.waitForURL(/\/libraries/, { timeout: 20_000 }),
    form.getByRole('button', { name: /sign in/i }).click(),
  ]);
}

export async function goAdmin(page: Page) {
  await page.goto('/admin');
  await page.waitForLoadState('networkidle');
}

export async function createLibraryViaBrowse(page: Page, libraryName: string) {
  await goAdmin(page);
  const createLibrarySection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Create Library' }),
  });
  await expect(createLibrarySection).toBeVisible({ timeout: 15_000 });
  await createLibrarySection.locator('input[placeholder="Name"]').first().fill(libraryName);
  await createLibrarySection.getByRole('button', { name: 'Browse', exact: true }).click();
  await expect(page.getByText('Directory selected')).toBeVisible();
  await createLibrarySection.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByText('Library created')).toBeVisible();
}

export async function triggerScan(page: Page, libraryName: string) {
  const libRow = page.locator('div', { hasText: libraryName }).first();
  await libRow.getByRole('button', { name: 'Scan' }).click();
  await expect(page.getByText('Scan started')).toBeVisible();
}
