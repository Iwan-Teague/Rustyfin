'use client';

import { apiFetch, apiJson, extractErrorMessage, parseResponseBody } from './api';

export type DownloadArtifactAvailability = 'available' | 'unavailable' | 'planned';

export type DownloadArtifact = {
  id: string;
  title: string;
  summary: string;
  availability: DownloadArtifactAvailability;
  detail: string;
  version?: string | null;
  package_filename?: string | null;
  download_path?: string | null;
  install_mode?: string | null;
  setup_path?: string | null;
  requires_sign_in: boolean;
  install_steps: string[];
};

export type DownloadCatalogResponse = {
  items: DownloadArtifact[];
};

export async function getDownloadsCatalog(): Promise<DownloadCatalogResponse> {
  return apiJson<DownloadCatalogResponse>('/downloads/catalog');
}

function parseDownloadFilename(contentDisposition: string | null, fallback: string): string {
  if (!contentDisposition) {
    return fallback;
  }
  const encodedMatch = contentDisposition.match(/filename\*=UTF-8''([^;]+)/i);
  if (encodedMatch?.[1]) {
    try {
      return decodeURIComponent(encodedMatch[1]);
    } catch {
      return encodedMatch[1];
    }
  }
  const plainMatch = contentDisposition.match(/filename="?([^\";]+)"?/i);
  return plainMatch?.[1] || fallback;
}

export async function downloadCatalogArtifactPackage(
  artifact: DownloadArtifact,
): Promise<{ blob: Blob; filename: string }> {
  if (!artifact.download_path) {
    throw new Error('This download is not currently available.');
  }
  const fallbackFilename = artifact.package_filename || `${artifact.id}.zip`;
  const response = await apiFetch(artifact.download_path);
  if (!response.ok) {
    const body = await parseResponseBody(response);
    throw new Error(extractErrorMessage(body, `Download failed: ${response.status}`));
  }
  const blob = await response.blob();
  return {
    blob,
    filename: parseDownloadFilename(response.headers.get('content-disposition'), fallbackFilename),
  };
}
