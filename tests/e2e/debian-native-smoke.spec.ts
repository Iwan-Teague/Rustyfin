import { expect, test } from '@playwright/test';
import {
  ADMIN,
  createLibraryViaApi,
  loginViaApi,
  triggerScanViaApi,
  waitForPlayableItemViaApi,
} from './helpers';

test('@debian-native-smoke login, channels, rooms, servers, and playback stay healthy', async ({ page }) => {
  const { page: authedPage, token } = await loginViaApi(page, ADMIN.username, ADMIN.password);
  const authHeaders = { Authorization: `Bearer ${token}` };

  await authedPage.goto('/');
  await expect(authedPage.getByRole('link', { name: 'Rustyfin' }).first()).toBeVisible({
    timeout: 20_000,
  });

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
  const playableItem = await waitForPlayableItemViaApi(authedPage, token, libraryId);

  const descriptorResponse = await authedPage.request.get(`/api/v1/items/${playableItem.id}/playback`, {
    headers: authHeaders,
  });
  expect(descriptorResponse.ok()).toBeTruthy();
  const descriptor = (await descriptorResponse.json()) as {
    file_id?: unknown;
    direct_url?: unknown;
    hls_start_url?: unknown;
    media_info_url?: unknown;
  };
  expect(typeof descriptor.file_id).toBe('string');
  expect(typeof descriptor.direct_url).toBe('string');
  expect(typeof descriptor.hls_start_url).toBe('string');
  expect(typeof descriptor.media_info_url).toBe('string');

  const mediaInfoResponse = await authedPage.request.get(descriptor.media_info_url as string, {
    headers: authHeaders,
  });
  expect(mediaInfoResponse.ok()).toBeTruthy();

  const directResponse = await authedPage.request.get(descriptor.direct_url as string);
  expect([200, 206]).toContain(directResponse.status());

  const hlsSessionResponse = await authedPage.request.post(descriptor.hls_start_url as string, {
    headers: authHeaders,
    data: {
      file_id: descriptor.file_id,
    },
  });
  expect(hlsSessionResponse.ok()).toBeTruthy();
  const hlsSession = (await hlsSessionResponse.json()) as {
    session_id?: unknown;
    hls_url?: unknown;
  };
  expect(typeof hlsSession.session_id).toBe('string');
  expect(typeof hlsSession.hls_url).toBe('string');

  const playlistResponse = await authedPage.request.get(hlsSession.hls_url as string, {
    headers: authHeaders,
  });
  expect(playlistResponse.ok()).toBeTruthy();
  const playlistText = await playlistResponse.text();
  expect(playlistText).toContain('#EXTM3U');

  const firstSegment = playlistText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => /seg_\d+\.ts(?:\?.*)?$/.test(line));
  expect(firstSegment).toBeTruthy();

  const segmentUrl = new URL(firstSegment!, new URL(hlsSession.hls_url as string, authedPage.url())).toString();
  const segmentResponse = await authedPage.request.get(segmentUrl, {
    headers: authHeaders,
  });
  expect([200, 206]).toContain(segmentResponse.status());

  await authedPage.request.post(`/api/v1/playback/sessions/${hlsSession.session_id}/stop`, {
    headers: authHeaders,
  });
});
