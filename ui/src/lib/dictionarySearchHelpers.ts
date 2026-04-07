'use client';

export function shouldUseBackendDictionarySearch(query: string): boolean {
  return query.trim().length > 0;
}

export function debounceDelayForDictionarySearch(query: string): number {
  const trimmedLength = query.trim().length;
  if (trimmedLength >= 8) return 180;
  if (trimmedLength >= 3) return 220;
  return 280;
}
