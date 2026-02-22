#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const configCandidates = [
  '.eslintrc',
  '.eslintrc.js',
  '.eslintrc.cjs',
  '.eslintrc.json',
  'eslint.config.js',
  'eslint.config.cjs',
  'eslint.config.mjs',
];

const hasConfig = configCandidates.some((name) =>
  fs.existsSync(path.join(process.cwd(), name)),
);

let hasEslint = true;
try {
  require.resolve('eslint');
} catch (_error) {
  hasEslint = false;
}

if (!hasEslint || !hasConfig) {
  console.log('[info] Skipping UI lint: eslint or config missing');
  process.exit(0);
}

const result = spawnSync('next', ['lint'], { stdio: 'inherit', shell: true });
process.exit(result.status ?? 1);
