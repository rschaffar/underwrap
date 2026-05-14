import { spawn } from 'node:child_process';

import { startBackendStub } from './backend-stub.ts';

const appPort = Number(process.env.ADMIN_UI_E2E_PORT ?? '3100');
const backendPort = Number(process.env.ADMIN_UI_E2E_BACKEND_PORT ?? '4011');

async function waitFor(url: string, timeoutMs: number) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(url);
      if (response.ok || response.status === 200) {
        return;
      }
    } catch {
      // retry
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`timed out waiting for ${url}`);
}

const backend = await startBackendStub(backendPort);
const app = spawn('npm', ['run', 'dev', '--', '--hostname', '127.0.0.1', '--port', String(appPort)], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    HOSTNAME: '127.0.0.1',
    PORT: String(appPort),
  },
  stdio: 'inherit',
});

async function shutdown(code = 0) {
  app.kill('SIGTERM');
  await backend.close().catch(() => undefined);
  process.exit(code);
}

process.on('SIGINT', () => {
  void shutdown(130);
});
process.on('SIGTERM', () => {
  void shutdown(0);
});
app.on('exit', (code) => {
  void backend.close().finally(() => process.exit(code ?? 0));
});

await waitFor(`http://127.0.0.1:${appPort}/login`, 120_000);

await new Promise(() => {});
