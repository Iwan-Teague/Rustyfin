import fs from 'node:fs/promises';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const ts = require('../../../ui/node_modules/typescript/lib/typescript.js');

const EXTENSION_ROOT = path.resolve(new URL('..', import.meta.url).pathname);
const TMP_DIR = path.join(EXTENSION_ROOT, '.tmp-build');
const DIST_DIR = path.join(EXTENSION_ROOT, 'dist');
const TARGETS = process.env.BROWSER ? [process.env.BROWSER] : ['chromium', 'firefox'];

async function rmrf(target) {
  await fs.rm(target, { recursive: true, force: true });
}

async function mkdirp(target) {
  await fs.mkdir(target, { recursive: true });
}

async function copyRecursive(from, to) {
  const stat = await fs.stat(from);
  if (stat.isDirectory()) {
    await mkdirp(to);
    const entries = await fs.readdir(from);
    for (const entry of entries) {
      await copyRecursive(path.join(from, entry), path.join(to, entry));
    }
    return;
  }
  await mkdirp(path.dirname(to));
  await fs.copyFile(from, to);
}

async function buildTypescript() {
  const configPath = ts.findConfigFile(EXTENSION_ROOT, ts.sys.fileExists, 'tsconfig.json');
  if (!configPath) {
    throw new Error('Missing tsconfig.json for rustyvault-webext');
  }
  const configFile = ts.readConfigFile(configPath, ts.sys.readFile);
  if (configFile.error) {
    throw new Error(ts.flattenDiagnosticMessageText(configFile.error.messageText, '\n'));
  }
  const parsed = ts.parseJsonConfigFileContent(configFile.config, ts.sys, EXTENSION_ROOT);
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
  });
  const result = program.emit();
  const diagnostics = ts.getPreEmitDiagnostics(program).concat(result.diagnostics);
  if (diagnostics.length > 0) {
    const lines = diagnostics.map((diagnostic) => {
      const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n');
      if (diagnostic.file && typeof diagnostic.start === 'number') {
        const { line, character } =
          diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
        return `${diagnostic.file.fileName}:${line + 1}:${character + 1} ${message}`;
      }
      return message;
    });
    throw new Error(lines.join('\n'));
  }
}

async function buildManifest(target) {
  const base = JSON.parse(
    await fs.readFile(path.join(EXTENSION_ROOT, 'manifest.base.json'), 'utf8'),
  );
  const overlay = JSON.parse(
    await fs.readFile(path.join(EXTENSION_ROOT, `manifest.${target}.json`), 'utf8'),
  );
  return {
    ...base,
    ...overlay,
  };
}

async function writeTarget(target) {
  const targetDir = path.join(DIST_DIR, target);
  await mkdirp(targetDir);
  await copyRecursive(path.join(TMP_DIR, 'src'), path.join(targetDir, 'src'));
  await copyRecursive(
    path.join(EXTENSION_ROOT, 'shared', 'vendor'),
    path.join(targetDir, 'src', 'shared', 'vendor'),
  );
  await copyRecursive(path.join(EXTENSION_ROOT, 'popup.css'), path.join(targetDir, 'popup.css'));
  await copyRecursive(path.join(EXTENSION_ROOT, 'popup.html'), path.join(targetDir, 'popup.html'));
  await copyRecursive(path.join(EXTENSION_ROOT, 'options.html'), path.join(targetDir, 'options.html'));
  await copyRecursive(path.join(EXTENSION_ROOT, 'README.md'), path.join(targetDir, 'README.md'));
  const manifest = await buildManifest(target);
  await fs.writeFile(
    path.join(targetDir, 'manifest.json'),
    JSON.stringify(manifest, null, 2) + '\n',
    'utf8',
  );
}

async function main() {
  await rmrf(TMP_DIR);
  await mkdirp(TMP_DIR);
  await rmrf(DIST_DIR);
  await mkdirp(DIST_DIR);
  await buildTypescript();
  for (const target of TARGETS) {
    await writeTarget(target);
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
