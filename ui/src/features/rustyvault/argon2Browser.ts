'use client';

type Argon2BrowserModule = {
  ArgonType: {
    Argon2d: number;
    Argon2i: number;
    Argon2id: number;
  };
  hash(params: {
    pass: string | Uint8Array;
    salt: string | Uint8Array;
    time?: number;
    mem?: number;
    hashLen?: number;
    parallelism?: number;
    secret?: Uint8Array;
    ad?: Uint8Array;
    type?: number;
  }): Promise<{
    hash: Uint8Array;
    hashHex: string;
    encoded: string;
  }>;
};

const ARGON2_BUNDLE_SRC = '/vendor/rustyvault/argon2-bundled.min.js';
const ARGON2_LOAD_TIMEOUT_MS = 8_000;

declare global {
  interface Window {
    argon2?: Argon2BrowserModule;
    __rustyVaultArgon2Load?: Promise<Argon2BrowserModule>;
  }
}

function resolveArgon2Browser(): Argon2BrowserModule {
  const candidate = window.argon2;
  if (
    !candidate ||
    typeof candidate.hash !== 'function' ||
    !candidate.ArgonType ||
    typeof candidate.ArgonType.Argon2id !== 'number'
  ) {
    throw new Error('Argon2 browser fallback did not initialize correctly');
  }
  return candidate;
}

async function loadArgon2Browser(): Promise<Argon2BrowserModule> {
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    throw new Error('Argon2 browser fallback can only load in a browser context');
  }
  if (window.argon2) {
    return resolveArgon2Browser();
  }
  if (!window.__rustyVaultArgon2Load) {
    window.__rustyVaultArgon2Load = new Promise<Argon2BrowserModule>((resolve, reject) => {
      let settled = false;
      const settleResolve = (value: Argon2BrowserModule) => {
        if (settled) return;
        settled = true;
        resolve(value);
      };
      const settleReject = (error: Error) => {
        if (settled) return;
        settled = true;
        reject(error);
      };
      const timeout = window.setTimeout(() => {
        settleReject(new Error('Timed out while loading the Argon2 browser fallback bundle'));
      }, ARGON2_LOAD_TIMEOUT_MS);
      const clearTimer = () => window.clearTimeout(timeout);
      const existing = document.querySelector<HTMLScriptElement>(
        `script[data-rustyvault-argon2="true"]`,
      );
      if (existing) {
        const existingState = existing.dataset.rustyvaultArgon2State;
        if (existingState === 'ready' || window.argon2) {
          clearTimer();
          try {
            settleResolve(resolveArgon2Browser());
          } catch (error) {
            settleReject(
              error instanceof Error
                ? error
                : new Error('Argon2 browser fallback did not initialize correctly'),
            );
          }
          return;
        }
        if (existingState === 'error') {
          clearTimer();
          settleReject(new Error('Failed to load Argon2 browser fallback bundle'));
          return;
        }
        existing.addEventListener(
          'load',
          () => {
            clearTimer();
            try {
              settleResolve(resolveArgon2Browser());
            } catch (error) {
              settleReject(
                error instanceof Error
                  ? error
                  : new Error('Argon2 browser fallback did not initialize correctly'),
              );
            }
          },
          { once: true },
        );
        existing.addEventListener(
          'error',
          () => {
            clearTimer();
            existing.dataset.rustyvaultArgon2State = 'error';
            settleReject(new Error('Failed to load Argon2 browser fallback bundle'));
          },
          { once: true },
        );
        return;
      }

      const script = document.createElement('script');
      script.src = ARGON2_BUNDLE_SRC;
      script.async = true;
      script.dataset.rustyvaultArgon2 = 'true';
      script.dataset.rustyvaultArgon2State = 'loading';
      script.onload = () => {
        clearTimer();
        try {
          script.dataset.rustyvaultArgon2State = 'ready';
          settleResolve(resolveArgon2Browser());
        } catch (error) {
          script.dataset.rustyvaultArgon2State = 'error';
          settleReject(
            error instanceof Error
              ? error
              : new Error('Argon2 browser fallback did not initialize correctly'),
          );
        }
      };
      script.onerror = () => {
        clearTimer();
        script.dataset.rustyvaultArgon2State = 'error';
        settleReject(new Error('Failed to load Argon2 browser fallback bundle'));
      };
      document.head.appendChild(script);
    }).catch((error) => {
      window.__rustyVaultArgon2Load = undefined;
      throw error;
    });
  }
  return window.__rustyVaultArgon2Load;
}

export async function deriveArgon2IdHashBytes(params: {
  pass: string | Uint8Array;
  salt: string | Uint8Array;
  time: number;
  mem: number;
  parallelism: number;
  hashLen: number;
}): Promise<Uint8Array> {
  const argon2 = await loadArgon2Browser();
  const result = await argon2.hash({
    pass: params.pass,
    salt: params.salt,
    time: params.time,
    mem: params.mem,
    parallelism: params.parallelism,
    hashLen: params.hashLen,
    type: argon2.ArgonType.Argon2id,
  });
  return result.hash;
}

export async function probeArgon2BrowserFallback(): Promise<void> {
  await loadArgon2Browser();
}
