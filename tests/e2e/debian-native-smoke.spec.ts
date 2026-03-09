import { expect, test } from '@playwright/test';
import { ADMIN, createLibraryViaBrowse, loginViaApi, triggerScan } from './helpers';

test('@debian-native-smoke login, channels, rooms, servers, and playback stay healthy', async ({ page }) => {
  await loginViaApi(page, ADMIN.username, ADMIN.password);

  await page.goto('/channels');
  await page.waitForLoadState('networkidle');
  await expect(page.getByText('Text Channels')).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText('Voice Channels')).toBeVisible({ timeout: 20_000 });

  await page.goto('/rooms');
  await page.waitForLoadState('networkidle');
  await expect(page.getByRole('heading', { name: 'Open Rooms' })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole('button', { name: 'Create Room', exact: true })).toBeVisible({
    timeout: 20_000,
  });

  await page.goto('/servers');
  await page.waitForLoadState('networkidle');
  await expect(page.getByRole('heading', { name: 'Known servers' })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole('heading', { name: 'Create Minecraft server' })).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByRole('heading', { name: 'Server management' })).toBeVisible({
    timeout: 20_000,
  });

  const libName = 'Debian Browser Smoke Fixtures';
  await createLibraryViaBrowse(page, libName);
  await triggerScan(page, libName);

  await page.goto('/libraries');
  await page.waitForLoadState('networkidle');

  const targetLib = page.getByRole('link', { name: libName }).first();
  await expect(targetLib).toBeVisible({ timeout: 30_000 });
  await targetLib.click();

  await expect
    .poll(
      async () => {
        await page.reload();
        await page.waitForLoadState('networkidle');
        return page.locator('a[href^="/items/"]').count();
      },
      { timeout: 60_000 }
    )
    .toBeGreaterThan(0);

  await page.locator('a[href^="/items/"]').first().click();
  await expect(page.getByRole('link', { name: 'Play Now' })).toBeVisible({ timeout: 20_000 });
  await page.getByRole('link', { name: 'Play Now' }).click();
  await expect(page).toHaveURL(/\/player\//);

  const directReq = page.waitForRequest(
    (req) => req.method() === 'GET' && req.url().includes('/stream/file/'),
    { timeout: 20_000 }
  );
  await page.getByRole('button', { name: 'Direct Play', exact: true }).click();
  await directReq;

  const hlsSessionReq = page.waitForRequest(
    (req) => req.method() === 'POST' && req.url().includes('/api/v1/playback/sessions'),
    { timeout: 20_000 }
  );
  const playlistReq = page.waitForRequest(
    (req) =>
      req.method() === 'GET' &&
      req.url().includes('/stream/hls/') &&
      req.url().includes('master.m3u8'),
    { timeout: 30_000 }
  );
  const segmentReq = page.waitForRequest(
    (req) =>
      req.method() === 'GET' &&
      req.url().includes('/stream/hls/') &&
      /seg_\\d+\\.ts/.test(req.url()),
    { timeout: 30_000 }
  );

  await page.getByRole('button', { name: 'Transcode (HLS)', exact: true }).click();
  await hlsSessionReq;
  await playlistReq;
  await segmentReq;
});
