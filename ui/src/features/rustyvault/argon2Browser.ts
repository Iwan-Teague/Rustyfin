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
      const existing = document.querySelector<HTMLScriptElement>(
        `script[data-rustyvault-argon2="true"]`,
      );
      if (existing) {
        existing.addEventListener(
          'load',
          () => {
            try {
              resolve(resolveArgon2Browser());
            } catch (error) {
              reject(error);
            }
          },
          { once: true },
        );
        existing.addEventListener(
          'error',
          () => reject(new Error('Failed to load Argon2 browser fallback bundle')),
          { once: true },
        );
        return;
      }

      const script = document.createElement('script');
      script.src = ARGON2_BUNDLE_SRC;
      script.async = true;
      script.dataset.rustyvaultArgon2 = 'true';
      script.onload = () => {
        try {
          resolve(resolveArgon2Browser());
        } catch (error) {
          reject(error);
        }
      };
      script.onerror = () => {
        reject(new Error('Failed to load Argon2 browser fallback bundle'));
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
  await deriveArgon2IdHashBytes({
    pass: 'probe',
    salt: new Uint8Array(16),
    time: 1,
    mem: 8_192,
    parallelism: 1,
    hashLen: 32,
  });
}
