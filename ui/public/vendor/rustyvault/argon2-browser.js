const ARGON2_WASM_URL = new URL('./argon2.wasm', import.meta.url).toString();

export const ArgonType = {
  Argon2d: 0,
  Argon2i: 1,
  Argon2id: 2,
};

let modulePromise = null;

function createWasmMemory(mem) {
  const KB = 1024;
  const MB = 1024 * KB;
  const GB = 1024 * MB;
  const WASM_PAGE_SIZE = 64 * KB;
  const totalMemory = (2 * GB - 64 * KB) / WASM_PAGE_SIZE;
  const initialMemory = Math.min(
    Math.max(Math.ceil((mem * KB) / WASM_PAGE_SIZE), 256) + 256,
    totalMemory,
  );
  return new WebAssembly.Memory({
    initial: initialMemory,
    maximum: totalMemory,
  });
}

async function loadWasmBinary() {
  const response = await fetch(ARGON2_WASM_URL);
  if (!response.ok) {
    throw new Error(`Failed to load Argon2 wasm (${response.status})`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

async function loadModule(mem = 1024) {
  if (modulePromise) {
    return modulePromise;
  }
  modulePromise = (async () => {
    const wasmBinary = await loadWasmBinary();
    return new Promise((resolve, reject) => {
      globalThis.Module = {
        wasmBinary,
        wasmMemory: createWasmMemory(mem),
        locateFile(path) {
          return path === 'argon2.wasm' ? ARGON2_WASM_URL : path;
        },
        onAbort(reason) {
          reject(new Error(String(reason || 'Argon2 wasm initialization failed')));
        },
        postRun() {
          resolve(globalThis.Module);
        },
      };
      import('./argon2.js').catch((error) => {
        modulePromise = null;
        reject(error);
      });
    });
  })().catch((error) => {
    modulePromise = null;
    throw error;
  });
  return modulePromise;
}

function allocateArray(module, arr) {
  return module.allocate(arr, 'i8', module.ALLOC_NORMAL);
}

function allocateArrayStr(module, arr) {
  return allocateArray(module, new Uint8Array([...arr, 0]));
}

function encodeUtf8(value) {
  if (typeof value !== 'string') {
    return value;
  }
  return new TextEncoder().encode(value);
}

export async function hash(params) {
  const mCost = params.mem || 1024;
  const module = await loadModule(mCost);
  const tCost = params.time || 1;
  const parallelism = params.parallelism || 1;
  const password = encodeUtf8(params.pass);
  const passwordPtr = allocateArrayStr(module, password);
  const salt = encodeUtf8(params.salt);
  const saltPtr = allocateArrayStr(module, salt);
  const hashLen = params.hashLen || 24;
  const hashPtr = module.allocate(new Array(hashLen), 'i8', module.ALLOC_NORMAL);
  const secretPtr = params.secret ? allocateArray(module, params.secret) : 0;
  const secretLen = params.secret ? params.secret.byteLength : 0;
  const adPtr = params.ad ? allocateArray(module, params.ad) : 0;
  const adLen = params.ad ? params.ad.byteLength : 0;
  const argon2Type = params.type ?? ArgonType.Argon2d;
  const encodedLen = module._argon2_encodedlen(
    tCost,
    mCost,
    parallelism,
    salt.length,
    hashLen,
    argon2Type,
  );
  const encodedPtr = module.allocate(new Array(encodedLen + 1), 'i8', module.ALLOC_NORMAL);
  const version = 0x13;
  let err = null;
  let res = 0;
  try {
    res = module._argon2_hash_ext(
      tCost,
      mCost,
      parallelism,
      passwordPtr,
      password.length,
      saltPtr,
      salt.length,
      hashPtr,
      hashLen,
      encodedPtr,
      encodedLen,
      argon2Type,
      secretPtr,
      secretLen,
      adPtr,
      adLen,
      version,
    );
  } catch (error) {
    err = error;
  }

  try {
    if (res !== 0 && !err) {
      err = new Error(module.UTF8ToString(module._argon2_error_message(res)));
    }
    if (err) {
      throw err;
    }
    const hashBytes = new Uint8Array(hashLen);
    let hashHex = '';
    for (let index = 0; index < hashLen; index += 1) {
      const byte = module.HEAP8[hashPtr + index];
      hashBytes[index] = byte;
      hashHex += (`0${(0xff & byte).toString(16)}`).slice(-2);
    }
    return {
      hash: hashBytes,
      hashHex,
      encoded: module.UTF8ToString(encodedPtr),
    };
  } finally {
    try {
      module._free(passwordPtr);
      module._free(saltPtr);
      module._free(hashPtr);
      module._free(encodedPtr);
      if (secretPtr) {
        module._free(secretPtr);
      }
      if (adPtr) {
        module._free(adPtr);
      }
    } catch {
      // ignore runtime free failures
    }
  }
}

export async function probeArgon2BrowserFallback() {
  await hash({
    pass: 'probe',
    salt: new Uint8Array(16),
    time: 1,
    mem: 8_192,
    parallelism: 1,
    hashLen: 32,
    type: ArgonType.Argon2id,
  });
}
