'use client';

import { apiJson } from '@/lib/api';

import { RUSTYVAULT_ACCESS_HEADER, type RustyVaultUriMatchMode } from './api';
import type {
  PasswordGeneratorOptions,
  PasswordGeneratorPreset,
} from './passwordGenerator';

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
  password_generator_default_preset: PasswordGeneratorPreset;
  password_generator_default_length: number;
  password_generator_include_uppercase: boolean;
  password_generator_include_lowercase: boolean;
  password_generator_include_numbers: boolean;
  password_generator_include_symbols: boolean;
  password_generator_exclude_ambiguous: boolean;
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
    password_generator_default_preset: 'balanced',
    password_generator_default_length: 22,
    password_generator_include_uppercase: true,
    password_generator_include_lowercase: true,
    password_generator_include_numbers: true,
    password_generator_include_symbols: true,
    password_generator_exclude_ambiguous: true,
  };
}

function normalizeMatchMode(raw: unknown): RustyVaultUriMatchMode {
  return raw === 'exact' || raw === 'host' || raw === 'never' ? raw : 'base_domain';
}

function normalizePasswordGeneratorPreset(raw: unknown): PasswordGeneratorPreset {
  return raw === 'memorable' || raw === 'maximum' ? raw : 'balanced';
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
    password_generator_default_preset: normalizePasswordGeneratorPreset(
      raw?.password_generator_default_preset,
    ),
    password_generator_default_length:
      typeof raw?.password_generator_default_length === 'number'
        ? Math.max(12, Math.min(64, raw.password_generator_default_length))
        : defaults.password_generator_default_length,
    password_generator_include_uppercase:
      typeof raw?.password_generator_include_uppercase === 'boolean'
        ? raw.password_generator_include_uppercase
        : defaults.password_generator_include_uppercase,
    password_generator_include_lowercase:
      typeof raw?.password_generator_include_lowercase === 'boolean'
        ? raw.password_generator_include_lowercase
        : defaults.password_generator_include_lowercase,
    password_generator_include_numbers:
      typeof raw?.password_generator_include_numbers === 'boolean'
        ? raw.password_generator_include_numbers
        : defaults.password_generator_include_numbers,
    password_generator_include_symbols:
      typeof raw?.password_generator_include_symbols === 'boolean'
        ? raw.password_generator_include_symbols
        : defaults.password_generator_include_symbols,
    password_generator_exclude_ambiguous:
      typeof raw?.password_generator_exclude_ambiguous === 'boolean'
        ? raw.password_generator_exclude_ambiguous
        : defaults.password_generator_exclude_ambiguous,
  };
}

export function passwordGeneratorDefaultsFromPrefs(
  prefs: RustyVaultPreferences,
): { preset: PasswordGeneratorPreset; options: PasswordGeneratorOptions } {
  return {
    preset: prefs.password_generator_default_preset,
    options: {
      length: prefs.password_generator_default_length,
      include_uppercase: prefs.password_generator_include_uppercase,
      include_lowercase: prefs.password_generator_include_lowercase,
      include_numbers: prefs.password_generator_include_numbers,
      include_symbols: prefs.password_generator_include_symbols,
      exclude_ambiguous: prefs.password_generator_exclude_ambiguous,
    },
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
