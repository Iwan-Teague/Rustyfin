import { expect, test } from '@playwright/test';
import {
  ADMIN,
  createLibraryViaApi,
  loginViaApi,
  triggerScanViaApi,
  waitForLibraryItemsViaApi,
} from './helpers';

test('@debian-native-smoke login, channels, rooms, servers, and playback stay healthy', async ({ page }) => {
  const { page: authedPage, token } = await loginViaApi(page, ADMIN.username, ADMIN.password);
  const authHeaders = { Authorization: `Bearer ${token}` };

  const channelsResponse = await authedPage.request.get('/api/v1/channels', {
    headers: authHeaders,
  });
  expect(channelsResponse.ok()).toBeTruthy();
  expect(Array.isArray(await channelsResponse.json())).toBeTruthy();

  const roomInvitesResponse = await authedPage.request.get('/api/v1/watch-party/invites', {
    headers: authHeaders,
  });
  expect(roomInvitesResponse.ok()).toBeTruthy();
  expect(Array.isArray(await roomInvitesResponse.json())).toBeTruthy();

  const serversResponse = await authedPage.request.get('/api/v1/servers/minecraft/instances', {
    headers: authHeaders,
  });
  expect(serversResponse.ok()).toBeTruthy();
  expect(Array.isArray(await serversResponse.json())).toBeTruthy();

  const libName = 'Debian Browser Smoke Fixtures';
  const libraryId = await createLibraryViaApi(authedPage, token, libName);
  await triggerScanViaApi(authedPage, token, libraryId);
  const items = await waitForLibraryItemsViaApi(authedPage, token, libraryId);
  const firstPlayableItem = items.find((item) => item.kind === 'movie' || item.kind === 'episode');
  expect(firstPlayableItem).toBeTruthy();

  await authedPage.goto(`/player/${firstPlayableItem!.id}`);
  await authedPage.waitForLoadState('networkidle');
  await expect(authedPage).toHaveURL(/\/player\//);
  await expect(authedPage.getByRole('button', { name: 'Direct Play', exact: true })).toBeVisible({
    timeout: 20_000,
  });

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
