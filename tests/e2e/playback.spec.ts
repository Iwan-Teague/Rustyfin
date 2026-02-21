import { expect, test } from '@playwright/test';
import { ADMIN, createLibraryViaBrowse, login, runSetupWizard, triggerScan } from './helpers';

test('@playback direct and HLS playback paths issue network requests', async ({ page }) => {
  await runSetupWizard(page);
  await login(page, ADMIN.username, ADMIN.password);

  const libName = 'Playback Fixtures';
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
    (req) =>
      req.method() === 'POST' && req.url().includes('/api/v1/playback/sessions'),
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
      req.url().match(/seg_\d+\.ts/),
    { timeout: 30_000 }
  );

  await page.getByRole('button', { name: 'Transcode (HLS)', exact: true }).click();
  await hlsSessionReq;
  await playlistReq;
  await segmentReq;
});
