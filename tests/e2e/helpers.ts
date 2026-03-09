import { expect, Page } from '@playwright/test';

const adminUsername = process.env.RUSTYFIN_ADMIN_USERNAME?.trim();
const adminPassword = process.env.RUSTYFIN_ADMIN_PASSWORD?.trim();
const userUsername = process.env.RUSTYFIN_USER_USERNAME?.trim();
const userPassword = process.env.RUSTYFIN_USER_PASSWORD?.trim();
const libraryPath = process.env.RUSTYFIN_TEST_LIBRARY_PATH?.trim();

export const ADMIN = {
  username: adminUsername && adminUsername.length > 0 ? adminUsername : 'admin',
  password: adminPassword && adminPassword.length > 0 ? adminPassword : 'AdminPassword123!' // >= 6 chars
};

export const USER = {
  username: userUsername && userUsername.length > 0 ? userUsername : 'basicuser',
  password: userPassword && userPassword.length > 0 ? userPassword : 'UserPassword123!' // >= 6 chars
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

  await form.getByRole('button', { name: /sign in/i }).click();

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const token = localStorage.getItem('token');
          return typeof token === 'string' && token.length > 0;
        }),
      { timeout: 40_000 }
    )
    .toBe(true);

  const errorNotice = page.locator('.notice-error').first();
  await expect(errorNotice).toHaveCount(0, { timeout: 5_000 });

  if (new URL(page.url()).pathname === '/login') {
    await page.goto('/');
  }

  await page.waitForLoadState('networkidle');
}

export async function loginViaApi(
  page: Page,
  username: string,
  password: string,
): Promise<{ page: Page; token: string }> {
  await page.goto('/login');
  await page.waitForLoadState('domcontentloaded');

  const response = await page.request.post('/api/v1/auth/login', {
    data: { username, password },
  });
  expect(response.ok()).toBeTruthy();

  const body = (await response.json()) as { token?: unknown };
  expect(typeof body.token).toBe('string');

  const token = body.token as string;
  await page.evaluate((value) => {
    localStorage.setItem('token', value);
  }, token);

  const meResponse = await page.request.get('/api/v1/users/me', {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });
  expect(meResponse.ok()).toBeTruthy();
  const meBody = (await meResponse.json()) as {
    id?: unknown;
    username?: unknown;
    role?: unknown;
    login_username?: unknown;
    avatar_url?: unknown;
  };

  await page.context().addInitScript((value) => {
    localStorage.setItem('token', value);
  }, token);
  await page.context().addInitScript((cachedMe) => {
    localStorage.setItem('rustfin_auth_me_v1', JSON.stringify(cachedMe));
  }, meBody);
  await page.evaluate(
    ([value, cachedMe]) => {
      localStorage.setItem('token', value);
      localStorage.setItem('rustfin_auth_me_v1', JSON.stringify(cachedMe));
    },
    [token, meBody] as const
  );

  const authedPage = await page.context().newPage();
  await authedPage.goto('/');
  await authedPage.waitForLoadState('domcontentloaded');
  await page.close();
  return { page: authedPage, token };
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

export async function createLibraryViaApi(page: Page, token: string, libraryName: string): Promise<string> {
  if (!libraryPath) {
    throw new Error('RUSTYFIN_TEST_LIBRARY_PATH is required for API-based smoke library creation');
  }

  const response = await page.request.post('/api/v1/libraries', {
    headers: {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
    data: {
      name: libraryName,
      kind: 'movies',
      paths: [libraryPath],
      settings: {
        show_images: true,
        prefer_local_artwork: true,
        fetch_online_artwork: true,
        tmdb_store_in_media_dir: false,
        tmdb_sync_on_new_media: true,
        tmdb_sync_schedule: 'manual',
        tmdb_fetch_posters: true,
        tmdb_fetch_backdrops: true,
        tmdb_fetch_metadata: true,
        tmdb_fetch_reviews: false,
      },
    },
  });
  expect(response.ok()).toBeTruthy();

  const body = (await response.json()) as { id?: unknown };
  expect(typeof body.id).toBe('string');
  return body.id as string;
}

export async function triggerScan(page: Page, libraryName: string) {
  const libRow = page.locator('div', { hasText: libraryName }).first();
  await libRow.getByRole('button', { name: 'Scan' }).click();
  await expect(page.getByText('Scan started')).toBeVisible();
}

export async function triggerScanViaApi(page: Page, token: string, libraryId: string) {
  const response = await page.request.post(`/api/v1/libraries/${libraryId}/scan`, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });
  expect(response.ok()).toBeTruthy();
}

export async function waitForLibraryItemsViaApi(
  page: Page,
  token: string,
  libraryId: string,
  timeoutMs = 60_000,
): Promise<Array<{ id: string; title: string; kind: string }>> {
  const startedAt = Date.now();

  while (Date.now() - startedAt < timeoutMs) {
    const response = await page.request.get(`/api/v1/libraries/${libraryId}/items`, {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    });
    expect(response.ok()).toBeTruthy();

    const body = (await response.json()) as Array<{ id?: unknown; title?: unknown; kind?: unknown }>;
    const items = body.filter(
      (item): item is { id: string; title: string; kind: string } =>
        typeof item.id === 'string' &&
        typeof item.title === 'string' &&
        typeof item.kind === 'string',
    );

    if (items.length > 0) {
      return items;
    }

    await page.waitForTimeout(1_000);
  }

  throw new Error(`Timed out waiting for library items for library ${libraryId}`);
}

export async function waitForPlayableItemViaApi(
  page: Page,
  token: string,
  libraryId: string,
  timeoutMs = 60_000,
): Promise<{ id: string; title: string; kind: string }> {
  const startedAt = Date.now();

  while (Date.now() - startedAt < timeoutMs) {
    const items = await waitForLibraryItemsViaApi(page, token, libraryId, 5_000);
    const candidateItems = items.filter((item) => item.kind === 'movie' || item.kind === 'episode');

    for (const item of candidateItems) {
      const response = await page.request.get(`/api/v1/items/${item.id}/playback`, {
        headers: {
          Authorization: `Bearer ${token}`,
        },
      });
      if (response.ok()) {
        return item;
      }
    }

    await page.waitForTimeout(1_000);
  }

  throw new Error(`Timed out waiting for a playable item in library ${libraryId}`);
}
