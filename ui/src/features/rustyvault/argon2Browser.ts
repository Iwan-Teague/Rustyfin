'use client';

type Argon2BrowserModule = {
  allocate(slab: ArrayLike<number>, type: string, allocator: number): number;
  ALLOC_NORMAL: number;
  UTF8ToString(pointer: number): string;
  HEAP8: Int8Array;
  _free(pointer: number): void;
  _argon2_encodedlen(
    timeCost: number,
    memoryCost: number,
    parallelism: number,
    saltLength: number,
    hashLength: number,
    argon2Type: number,
  ): number;
  _argon2_hash_ext(
    timeCost: number,
    memoryCost: number,
    parallelism: number,
    passwordPointer: number,
    passwordLength: number,
    saltPointer: number,
    saltLength: number,
    hashPointer: number,
    hashLength: number,
    encodedPointer: number,
    encodedLength: number,
    argon2Type: number,
    secretPointer: number,
    secretLength: number,
    associatedDataPointer: number,
    associatedDataLength: number,
    version: number,
  ): number;
  _argon2_error_message(code: number): number;
};

const ARGON2_JS_SRC = '/vendor/rustyvault/argon2.js';
const ARGON2_WASM_URL = '/vendor/rustyvault/argon2.wasm';
const ARGON2_LOAD_TIMEOUT_MS = 8_000;
const ARGON2_VERSION = 0x13;
const ARGON2_TYPE_ID = 2;

declare global {
  interface Window {
    Module?: Partial<Argon2BrowserModule> & Record<string, unknown>;
    __rustyVaultArgon2ModuleLoad?: Promise<Argon2BrowserModule>;
  }
}

function resolveArgon2BrowserModule(): Argon2BrowserModule {
  const candidate = window.Module;
  if (
    !candidate ||
    typeof candidate.allocate !== 'function' ||
    typeof candidate.UTF8ToString !== 'function' ||
    !(candidate.HEAP8 instanceof Int8Array) ||
    typeof candidate._free !== 'function' ||
    typeof candidate._argon2_encodedlen !== 'function' ||
    typeof candidate._argon2_hash_ext !== 'function' ||
    typeof candidate._argon2_error_message !== 'function' ||
    typeof candidate.ALLOC_NORMAL !== 'number'
  ) {
    throw new Error('Argon2 browser runtime did not initialize correctly');
  }
  return candidate as Argon2BrowserModule;
}

function createWasmMemory(memoryKib: number) {
  const kib = 1024;
  const mib = 1024 * kib;
  const gib = 1024 * mib;
  const wasmPageSize = 64 * kib;
  const totalPages = (2 * gib - 64 * kib) / wasmPageSize;
  const initialPages = Math.min(
    Math.max(Math.ceil((memoryKib * kib) / wasmPageSize), 256) + 256,
    totalPages,
  );
  return new WebAssembly.Memory({
    initial: initialPages,
    maximum: totalPages,
  });
}

async function loadWasmBinary() {
  const controller = new AbortController();
  const timeoutId = window.setTimeout(() => controller.abort(), ARGON2_LOAD_TIMEOUT_MS);
  try {
    const response = await fetch(ARGON2_WASM_URL, {
      cache: 'force-cache',
      credentials: 'same-origin',
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error(`Failed to load Argon2 wasm (${response.status})`);
    }
    return new Uint8Array(await response.arrayBuffer());
  } finally {
    window.clearTimeout(timeoutId);
  }
}

async function loadArgon2Browser(memoryKib = 1024): Promise<Argon2BrowserModule> {
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    throw new Error('Argon2 browser fallback can only load in a browser context');
  }
  try {
    return resolveArgon2BrowserModule();
  } catch {
    // fall through
  }
  if (!window.__rustyVaultArgon2ModuleLoad) {
    window.__rustyVaultArgon2ModuleLoad = (async () => {
      const wasmBinary = await loadWasmBinary();
      return new Promise<Argon2BrowserModule>((resolve, reject) => {
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
        const timeoutId = window.setTimeout(() => {
          settleReject(new Error('Timed out while initializing the Argon2 browser runtime'));
        }, ARGON2_LOAD_TIMEOUT_MS);
        const clearTimer = () => window.clearTimeout(timeoutId);

        document
          .querySelectorAll('script[data-rustyvault-argon2-runtime="true"]')
          .forEach((node) => node.remove());

        window.Module = {
          wasmBinary,
          wasmMemory: createWasmMemory(memoryKib),
          locateFile(path: string) {
            return path === 'argon2.wasm' ? ARGON2_WASM_URL : path;
          },
          onAbort(reason: unknown) {
            clearTimer();
            settleReject(new Error(String(reason || 'Argon2 browser runtime aborted')));
          },
          postRun() {
            clearTimer();
            try {
              settleResolve(resolveArgon2BrowserModule());
            } catch (error) {
              settleReject(
                error instanceof Error
                  ? error
                  : new Error('Argon2 browser runtime did not initialize correctly'),
              );
            }
          },
        };

        const script = document.createElement('script');
        script.src = ARGON2_JS_SRC;
        script.async = true;
        script.dataset.rustyvaultArgon2Runtime = 'true';
        script.onerror = () => {
          clearTimer();
          settleReject(new Error('Failed to load the Argon2 browser runtime script'));
        };
        document.head.appendChild(script);
      });
    })().catch((error) => {
      window.__rustyVaultArgon2ModuleLoad = undefined;
      throw error;
    });
  }
  return window.__rustyVaultArgon2ModuleLoad;
}

function allocateArray(module: Argon2BrowserModule, value: Uint8Array) {
  return module.allocate(value, 'i8', module.ALLOC_NORMAL);
}

function allocateArrayString(module: Argon2BrowserModule, value: Uint8Array) {
  return allocateArray(module, new Uint8Array([...value, 0]));
}

function encodeUtf8(value: string | Uint8Array) {
  return typeof value === 'string' ? new TextEncoder().encode(value) : value;
}

async function hashArgon2(params: {
  pass: string | Uint8Array;
  salt: string | Uint8Array;
  time?: number;
  mem?: number;
  hashLen?: number;
  parallelism?: number;
}) {
  const memoryKib = params.mem || 1024;
  const module = await loadArgon2Browser(memoryKib);
  const timeCost = params.time || 1;
  const parallelism = params.parallelism || 1;
  const password = encodeUtf8(params.pass);
  const passwordPointer = allocateArrayString(module, password);
  const salt = encodeUtf8(params.salt);
  const saltPointer = allocateArrayString(module, salt);
  const hashLength = params.hashLen || 24;
  const hashPointer = module.allocate(new Array(hashLength), 'i8', module.ALLOC_NORMAL);
  const encodedLength = module._argon2_encodedlen(
    timeCost,
    memoryKib,
    parallelism,
    salt.length,
    hashLength,
    ARGON2_TYPE_ID,
  );
  const encodedPointer = module.allocate(new Array(encodedLength + 1), 'i8', module.ALLOC_NORMAL);
  let resultCode = 0;
  let error: Error | null = null;
  try {
    resultCode = module._argon2_hash_ext(
      timeCost,
      memoryKib,
      parallelism,
      passwordPointer,
      password.length,
      saltPointer,
      salt.length,
      hashPointer,
      hashLength,
      encodedPointer,
      encodedLength,
      ARGON2_TYPE_ID,
      0,
      0,
      0,
      0,
      ARGON2_VERSION,
    );
  } catch (cause) {
    error = cause instanceof Error ? cause : new Error(String(cause));
  }
  try {
    if (resultCode !== 0 && !error) {
      error = new Error(module.UTF8ToString(module._argon2_error_message(resultCode)));
    }
    if (error) {
      throw error;
    }
    const hash = new Uint8Array(hashLength);
    for (let index = 0; index < hashLength; index += 1) {
      hash[index] = module.HEAP8[hashPointer + index] & 0xff;
    }
    return { hash };
  } finally {
    try {
      module._free(passwordPointer);
      module._free(saltPointer);
      module._free(hashPointer);
      module._free(encodedPointer);
    } catch {
      // ignore runtime free failures
    }
  }
}

export async function deriveArgon2IdHashBytes(params: {
  pass: string | Uint8Array;
  salt: string | Uint8Array;
  time: number;
  mem: number;
  parallelism: number;
  hashLen: number;
}): Promise<Uint8Array> {
  const result = await hashArgon2(params);
  return result.hash;
}

export async function probeArgon2BrowserFallback(): Promise<void> {
  await loadArgon2Browser(8_192);
}
