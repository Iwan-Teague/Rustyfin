import type { ExtensionSettings, PagePolicy } from './types.js';

function normalizeHost(value: string): string {
  return (value || '').trim().toLowerCase().replace(/\.+$/, '');
}

function parseUrl(raw: string | undefined | null): URL | null {
  if (!raw) {
    return null;
  }
  try {
    const parsed = new URL(raw);
    if (!['http:', 'https:'].includes(parsed.protocol)) {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

export function isExcludedDomain(hostOrUrl: string, excludedDomains: string[] = []): boolean {
  const parsed = parseUrl(hostOrUrl);
  const normalizedHost = normalizeHost(parsed ? parsed.hostname : hostOrUrl);
  if (!normalizedHost) {
    return false;
  }
  return excludedDomains.some((domain) => {
    const normalizedDomain = normalizeHost(domain);
    return (
      Boolean(normalizedDomain) &&
      (normalizedHost === normalizedDomain || normalizedHost.endsWith(`.${normalizedDomain}`))
    );
  });
}

export function evaluatePagePolicy(
  input: { url?: string; topLevelUrl?: string; frameUrl?: string; isTopFrame?: boolean } = {},
  settings: Partial<ExtensionSettings> = {},
): PagePolicy {
  const currentUrl = parseUrl(input.url || input.frameUrl || input.topLevelUrl || '');
  const topLevelUrl = parseUrl(input.topLevelUrl || input.url || input.frameUrl || '');
  const effectiveUrl = currentUrl || topLevelUrl;
  const effectiveTopLevelUrl = topLevelUrl || currentUrl;
  const isTopFrame = input.isTopFrame !== false;
  const hostname = normalizeHost(effectiveUrl?.hostname || '');
  const topLevelHostname = normalizeHost(effectiveTopLevelUrl?.hostname || hostname);
  const excluded =
    isExcludedDomain(hostname, settings.excludedDomains || []) ||
    isExcludedDomain(topLevelHostname, settings.excludedDomains || []);
  const isHttp = effectiveUrl?.protocol === 'http:' || effectiveTopLevelUrl?.protocol === 'http:';
  const sameOriginIframe =
    !isTopFrame &&
    Boolean(effectiveUrl && effectiveTopLevelUrl && effectiveUrl.origin === effectiveTopLevelUrl.origin);
  const crossOriginIframe =
    !isTopFrame &&
    Boolean(effectiveUrl && effectiveTopLevelUrl && effectiveUrl.origin !== effectiveTopLevelUrl.origin);

  let lookupBlockedReason: string | null = null;
  let manualFillBlockedReason: string | null = null;
  let savePromptBlockedReason: string | null = null;

  if (!effectiveUrl || !effectiveTopLevelUrl) {
    lookupBlockedReason = 'unsupported_url';
    manualFillBlockedReason = 'unsupported_url';
    savePromptBlockedReason = 'unsupported_url';
  } else if (excluded) {
    lookupBlockedReason = 'excluded_domain';
    manualFillBlockedReason = 'excluded_domain';
    savePromptBlockedReason = 'excluded_domain';
  } else {
    if (crossOriginIframe) {
      lookupBlockedReason = 'untrusted_iframe';
      manualFillBlockedReason = 'untrusted_iframe';
      savePromptBlockedReason = 'untrusted_iframe';
    }
    if (isHttp && !settings.allowManualHttpFill) {
      lookupBlockedReason = lookupBlockedReason || 'http_blocked';
      manualFillBlockedReason = manualFillBlockedReason || 'http_blocked';
    }
  }

  const chips: string[] = [];
  if (excluded) chips.push('Excluded domain');
  if (isHttp && !settings.allowManualHttpFill) chips.push('HTTP blocked');
  if (crossOriginIframe && (settings.warnOnUntrustedIframe ?? true)) chips.push('Cross-origin iframe');
  if (sameOriginIframe) chips.push('Same-origin iframe');

  return {
    url: effectiveUrl ? effectiveUrl.toString() : null,
    topLevelUrl: effectiveTopLevelUrl ? effectiveTopLevelUrl.toString() : null,
    hostname,
    topLevelHostname,
    isTopFrame,
    isHttp,
    isExcluded: excluded,
    sameOriginIframe,
    crossOriginIframe,
    canLookup: !lookupBlockedReason,
    canManualFill: !manualFillBlockedReason,
    canSavePrompt: !savePromptBlockedReason,
    lookupBlockedReason,
    manualFillBlockedReason,
    savePromptBlockedReason,
    chips,
  };
}

export function describePolicyReason(reason: string | null | undefined): string {
  switch (reason) {
    case 'excluded_domain':
      return 'Vault prompts and autofill are suppressed on this excluded domain.';
    case 'http_blocked':
      return 'Manual fill is blocked on HTTP pages unless you explicitly allow it.';
    case 'untrusted_iframe':
      return 'Cross-origin iframe targets are blocked for manual fill and save prompts.';
    case 'unsupported_url':
      return 'This page URL is not supported by the vault extension.';
    default:
      return '';
  }
}
