import path from 'node:path';

import { defineConfig } from '@playwright/test';

const appPort = Number(process.env.ADMIN_UI_E2E_PORT ?? '3100');
const configPath = process.env.UNDERWRAP_ADMIN_UI_CONFIG || path.join(process.cwd(), 'dev-fixtures', 'e2e-admin-ui.yaml');

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  use: {
    baseURL: `http://127.0.0.1:${appPort}`,
    headless: true,
    launchOptions: {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || '/etc/profiles/per-user/robert/bin/google-chrome',
    },
  },
  webServer: {
    command: 'node --experimental-strip-types ./tests/e2e/start-stack.ts',
    url: `http://127.0.0.1:${appPort}/login`,
    reuseExistingServer: false,
    timeout: 120_000,
    cwd: process.cwd(),
    env: {
      ...process.env,
      ADMIN_UI_E2E_PORT: String(appPort),
      ADMIN_UI_E2E_BACKEND_PORT: process.env.ADMIN_UI_E2E_BACKEND_PORT || '4011',
      UNDERWRAP_ADMIN_UI_CONFIG: configPath,
    },
  },
});
