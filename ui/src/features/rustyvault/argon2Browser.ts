'use client';

type Argon2BrowserHashResult = {
  hash: Uint8Array;
  hashHex: string;
  encoded: string;
};

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
  }): Promise<Argon2BrowserHashResult>;
};

const ARGON2_MODULE_SRC = '/vendor/rustyvault/argon2-browser.js';
const ARGON2_LOAD_TIMEOUT_MS = 8_000;

let argon2ModulePromise: Promise<Argon2BrowserModule> | null = null;

async function withTimeout<T>(promise: Promise<T>, message: string, timeoutMs = ARGON2_LOAD_TIMEOUT_MS) {
  let timeoutId = 0;
  try {
    return await Promise.race<T>([
      promise,
      new Promise<T>((_, reject) => {
        timeoutId = window.setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    if (timeoutId) {
      window.clearTimeout(timeoutId);
    }
  }
}

async function loadArgon2BrowserModule(): Promise<Argon2BrowserModule> {
  if (typeof window === 'undefined') {
    throw new Error('Argon2 browser fallback can only load in a browser context');
  }
  if (!argon2ModulePromise) {
    argon2ModulePromise = withTimeout(
      import(/* webpackIgnore: true */ ARGON2_MODULE_SRC).then((module) => {
        if (
          !module ||
          !module.ArgonType ||
          typeof module.ArgonType.Argon2id !== 'number' ||
          typeof module.hash !== 'function'
        ) {
          throw new Error('Argon2 browser fallback module did not initialize correctly');
        }
        return module as Argon2BrowserModule;
      }),
      'Timed out while loading the Argon2 browser fallback module',
    ).catch((error) => {
      argon2ModulePromise = null;
      throw error;
    });
  }
  return argon2ModulePromise;
}

export async function deriveArgon2IdHashBytes(params: {
  pass: string | Uint8Array;
  salt: string | Uint8Array;
  time: number;
  mem: number;
  parallelism: number;
  hashLen: number;
}): Promise<Uint8Array> {
  const module = await loadArgon2BrowserModule();
  const result = await withTimeout(
    module.hash({
      pass: params.pass,
      salt: params.salt,
      time: params.time,
      mem: params.mem,
      parallelism: params.parallelism,
      hashLen: params.hashLen,
      type: module.ArgonType.Argon2id,
    }),
    'Timed out while deriving the Argon2 browser fallback hash',
    20_000,
  );
  return result.hash;
}

export async function probeArgon2BrowserFallback(): Promise<void> {
  await loadArgon2BrowserModule();
}
