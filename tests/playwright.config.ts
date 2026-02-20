import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';

const runDir = process.env.RUSTYFIN_TEST_RUN_DIR || path.join(process.cwd(), '_runs', 'adhoc');
const baseURL = process.env.RUSTYFIN_BASE_URL || 'http://localhost:3000';

export default defineConfig({
  testDir: './e2e',
  timeout: 90_000,
  expect: { timeout: 15_000 },
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,

  outputDir: path.join(runDir, 'playwright', 'artifacts'),

  reporter: [
    ['line'],
    ['json', { outputFile: path.join(runDir, 'playwright', 'results.json') }],
    ['junit', { outputFile: path.join(runDir, 'playwright', 'results.junit.xml') }],
    ['html', { outputFolder: path.join(runDir, 'playwright', 'html-report'), open: 'never' }],
  ],

  use: {
    baseURL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    viewport: { width: 1280, height: 800 },
  },

  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});

