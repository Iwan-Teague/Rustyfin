'use client';

import { apiJson } from '@/lib/api';

import { RUSTYVAULT_ACCESS_HEADER, type RustyVaultUriMatchMode } from './api';

export type RustyVaultPreferences = {
  auto_lock_minutes: number;
  clipboard_clear_seconds: number;
  inline_save_prompt_enabled: boolean;
  inline_autofill_enabled: boolean;
  default_match_mode: RustyVaultUriMatchMode;
  warn_on_http: boolean;
  warn_on_untrusted_iframe: boolean;
  excluded_domains: string[];
  allow_manual_http_fill: boolean;
};

export function defaultRustyVaultPreferences(): RustyVaultPreferences {
  return {
    auto_lock_minutes: 15,
    clipboard_clear_seconds: 30,
    inline_save_prompt_enabled: true,
    inline_autofill_enabled: true,
    default_match_mode: 'base_domain',
    warn_on_http: true,
    warn_on_untrusted_iframe: true,
    excluded_domains: [],
    allow_manual_http_fill: false,
  };
}

function normalizeMatchMode(raw: unknown): RustyVaultUriMatchMode {
  return raw === 'exact' || raw === 'host' || raw === 'never' ? raw : 'base_domain';
}

function normalizeRustyVaultPreferences(
  raw: Partial<RustyVaultPreferences> | null | undefined,
): RustyVaultPreferences {
  const defaults = defaultRustyVaultPreferences();
  return {
    auto_lock_minutes:
      typeof raw?.auto_lock_minutes === 'number'
        ? raw.auto_lock_minutes
        : defaults.auto_lock_minutes,
    clipboard_clear_seconds:
      typeof raw?.clipboard_clear_seconds === 'number'
        ? raw.clipboard_clear_seconds
        : defaults.clipboard_clear_seconds,
    inline_save_prompt_enabled:
      typeof raw?.inline_save_prompt_enabled === 'boolean'
        ? raw.inline_save_prompt_enabled
        : defaults.inline_save_prompt_enabled,
    inline_autofill_enabled:
      typeof raw?.inline_autofill_enabled === 'boolean'
        ? raw.inline_autofill_enabled
        : defaults.inline_autofill_enabled,
    default_match_mode: normalizeMatchMode(raw?.default_match_mode),
    warn_on_http:
      typeof raw?.warn_on_http === 'boolean' ? raw.warn_on_http : defaults.warn_on_http,
    warn_on_untrusted_iframe:
      typeof raw?.warn_on_untrusted_iframe === 'boolean'
        ? raw.warn_on_untrusted_iframe
        : defaults.warn_on_untrusted_iframe,
    excluded_domains: Array.isArray(raw?.excluded_domains)
      ? raw.excluded_domains.filter((value): value is string => typeof value === 'string')
      : defaults.excluded_domains,
    allow_manual_http_fill:
      typeof raw?.allow_manual_http_fill === 'boolean'
        ? raw.allow_manual_http_fill
        : defaults.allow_manual_http_fill,
  };
}

function rustyvaultHeaders(vaultAccessToken?: string | null) {
  const headers = new Headers();
  if (vaultAccessToken) {
    headers.set(RUSTYVAULT_ACCESS_HEADER, vaultAccessToken);
  }
  return headers;
}

export async function getMyRustyVaultPreferences(
  vaultAccessToken?: string | null,
): Promise<RustyVaultPreferences> {
  const prefs = await apiJson<Partial<RustyVaultPreferences>>('/vault/preferences', {
    headers: rustyvaultHeaders(vaultAccessToken),
  });
  return normalizeRustyVaultPreferences(prefs);
}

export async function updateMyRustyVaultPreferences(
  prefs: RustyVaultPreferences,
  vaultAccessToken?: string | null,
): Promise<RustyVaultPreferences> {
  const next = normalizeRustyVaultPreferences(prefs);
  const updated = await apiJson<Partial<RustyVaultPreferences>>('/vault/preferences', {
    method: 'PATCH',
    headers: rustyvaultHeaders(vaultAccessToken),
    body: JSON.stringify(next),
  });
  return normalizeRustyVaultPreferences(updated);
}
