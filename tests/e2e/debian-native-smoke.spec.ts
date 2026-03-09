import { expect, test } from '@playwright/test';
import { ADMIN, createLibraryViaBrowse, loginViaApi, triggerScan } from './helpers';

test('@debian-native-smoke login, channels, rooms, servers, and playback stay healthy', async ({ page }) => {
  const authedPage = await loginViaApi(page, ADMIN.username, ADMIN.password);

  await authedPage.goto('/channels');
  await authedPage.waitForLoadState('networkidle');
  await expect(authedPage.getByText('Loading…')).toHaveCount(0, { timeout: 40_000 });
  await expect(authedPage.getByText('Text Channels')).toBeVisible({ timeout: 40_000 });
  await expect(authedPage.getByText('Voice Channels')).toBeVisible({ timeout: 40_000 });

  await authedPage.goto('/rooms');
  await authedPage.waitForLoadState('networkidle');
  await expect(authedPage.getByRole('heading', { name: 'Open Rooms' })).toBeVisible({ timeout: 20_000 });
  await expect(authedPage.getByRole('button', { name: 'Create Room', exact: true })).toBeVisible({
    timeout: 20_000,
  });

  await authedPage.goto('/servers');
  await authedPage.waitForLoadState('networkidle');
  await expect(authedPage.getByRole('heading', { name: 'Known servers' })).toBeVisible({ timeout: 20_000 });
  await expect(authedPage.getByRole('heading', { name: 'Create Minecraft server' })).toBeVisible({
    timeout: 20_000,
  });
  await expect(authedPage.getByRole('heading', { name: 'Server management' })).toBeVisible({
    timeout: 20_000,
  });

  const libName = 'Debian Browser Smoke Fixtures';
  await createLibraryViaBrowse(authedPage, libName);
  await triggerScan(authedPage, libName);

  await authedPage.goto('/libraries');
  await authedPage.waitForLoadState('networkidle');

  const targetLib = authedPage.getByRole('link', { name: libName }).first();
  await expect(targetLib).toBeVisible({ timeout: 30_000 });
  await targetLib.click();

  await expect
    .poll(
      async () => {
        await authedPage.reload();
        await authedPage.waitForLoadState('networkidle');
        return authedPage.locator('a[href^="/items/"]').count();
      },
      { timeout: 60_000 }
    )
    .toBeGreaterThan(0);

  await authedPage.locator('a[href^="/items/"]').first().click();
  await expect(authedPage.getByRole('link', { name: 'Play Now' })).toBeVisible({ timeout: 20_000 });
  await authedPage.getByRole('link', { name: 'Play Now' }).click();
  await expect(authedPage).toHaveURL(/\/player\//);

  const directReq = authedPage.waitForRequest(
    (req) => req.method() === 'GET' && req.url().includes('/stream/file/'),
    { timeout: 20_000 }
  );
  await authedPage.getByRole('button', { name: 'Direct Play', exact: true }).click();
  await directReq;

  const hlsSessionReq = authedPage.waitForRequest(
    (req) => req.method() === 'POST' && req.url().includes('/api/v1/playback/sessions'),
    { timeout: 20_000 }
  );
  const playlistReq = authedPage.waitForRequest(
    (req) =>
      req.method() === 'GET' &&
      req.url().includes('/stream/hls/') &&
      req.url().includes('master.m3u8'),
    { timeout: 30_000 }
  );
  const segmentReq = authedPage.waitForRequest(
    (req) =>
      req.method() === 'GET' &&
      req.url().includes('/stream/hls/') &&
      /seg_\\d+\\.ts/.test(req.url()),
    { timeout: 30_000 }
  );

  await authedPage.getByRole('button', { name: 'Transcode (HLS)', exact: true }).click();
  await hlsSessionReq;
  await playlistReq;
  await segmentReq;
});
