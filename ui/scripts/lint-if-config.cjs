#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const packageRoot = path.resolve(__dirname, '..');

const configCandidates = [
  '.eslintrc',
  '.eslintrc.js',
  '.eslintrc.cjs',
  '.eslintrc.json',
  'eslint.config.js',
  'eslint.config.cjs',
  'eslint.config.mjs',
];

const hasConfig = configCandidates.some((name) => fs.existsSync(path.join(packageRoot, name)));

let hasEslint = true;
try {
  require.resolve('eslint', { paths: [packageRoot] });
} catch (_error) {
  hasEslint = false;
}

if (!hasConfig) {
  console.error('[error] UI lint config missing. Add an ESLint config file and retry.');
  process.exit(1);
}

if (!hasEslint) {
  console.error('[error] eslint dependency missing. Run `npm --prefix ui install` and retry.');
  process.exit(1);
}

const result = spawnSync('next', ['lint'], {
  stdio: 'inherit',
  shell: true,
  cwd: packageRoot,
});
process.exit(result.status ?? 1);
