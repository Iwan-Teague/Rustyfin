import { test, expect } from '@playwright/test';
import { runSetupWizard, ADMIN, USER, login, createLibraryViaBrowse } from './helpers';

test('@permissions non-admin must not access /admin (regression)', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  // Create a library first (simple users require at least one library).
  await createLibraryViaBrowse(page, 'Test Movies');

  const createUserSection = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Create User' }),
  });
  await expect(createUserSection).toBeVisible({ timeout: 15_000 });

  // Create a user
  await createUserSection.locator('input[placeholder="Username"]').first().fill(USER.username);
  await createUserSection.locator('input[placeholder="Password (min 12 chars)"]').first().fill(USER.password);

  // Assign the specific created library to this simple user.
  const allowedLibCheckbox = createUserSection
    .locator('label', { hasText: 'Test Movies' })
    .locator('input[type="checkbox"]');
  await expect(allowedLibCheckbox).toBeVisible({ timeout: 15_000 });
  await allowedLibCheckbox.check();
  await expect(allowedLibCheckbox).toBeChecked();

  // Submit create-user form (button text exists in current UI).
  const createUserResponsePromise = page.waitForResponse((resp) =>
    resp.request().method() === 'POST' &&
    resp.url().includes('/api/v1/users')
  );
  await createUserSection.getByRole('button', { name: 'Create User', exact: true }).click();
  const createUserResponse = await createUserResponsePromise;
  expect(
    createUserResponse.ok(),
    `Expected user creation request to succeed, got HTTP ${createUserResponse.status()}`
  ).toBe(true);

  await expect(page.getByText('User created')).toBeVisible({ timeout: 15_000 });
  await expect(
    page.locator('section').filter({
      has: page.getByRole('heading', { name: 'User Permissions' }),
    }).getByText(USER.username)
  ).toBeVisible({ timeout: 15_000 });
  await expect(createUserSection.locator('input[placeholder="Username"]').first()).toHaveValue('');

  // Desired: logout exists. If not implemented yet, this will fail (correctly).
  await page.getByRole('button', { name: 'Logout' }).click();
  await expect(page).toHaveURL(/\/login/);

  await login(page, USER.username, USER.password);

  // Admin should be hidden for non-admins.
  await expect(page.getByRole('link', { name: 'Admin' })).toHaveCount(0);

  // Direct /admin visit should redirect or deny.
  await page.goto('/admin');
  await expect(page).not.toHaveURL(/\/admin$/);
});
