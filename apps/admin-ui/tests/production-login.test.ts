import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { randomInt } from 'node:crypto';
import path from 'node:path';

async function waitFor(url: string, timeoutMs: number) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {
      // retry
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function withServer(
  envOverrides: Record<string, string>,
  run: (baseUrl: string) => Promise<void>,
) {
  const port = randomInt(4100, 4900);
  const root = process.cwd();
  const child = spawn('node', ['.next/standalone/server.js'], {
    cwd: root,
    env: {
      ...process.env,
      HOSTNAME: '127.0.0.1',
      PORT: String(port),
      UNDERWRAP_ADMIN_UI_CONFIG: path.join(root, 'dev-fixtures', 'admin-ui.yaml'),
      ...envOverrides,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  let stderr = '';
  child.stderr.on('data', (chunk) => {
    stderr += chunk.toString();
  });

  try {
    await waitFor(`http://127.0.0.1:${port}/login`, 30_000);
    await run(`http://127.0.0.1:${port}`);
  } finally {
    child.kill('SIGTERM');
    await once(child, 'exit').catch(() => undefined);
    if (child.exitCode && child.exitCode !== 0 && stderr) {
      throw new Error(stderr);
    }
  }
}

test('packaged server login route rejects wrong password, accepts correct password, and honors forwarded same-origin headers', async () => {
  await withServer({}, async (baseUrl) => {
    const loginPage = await fetch(`${baseUrl}/login`);
    assert.equal(loginPage.status, 200);
    assert.match(loginPage.headers.get('content-security-policy') ?? '', /default-src 'self'/);

    const wrongPassword = await fetch(`${baseUrl}/auth/login`, {
      method: 'POST',
      headers: {
        Origin: baseUrl,
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: new URLSearchParams({ username: 'alice', password: 'wrong-password' }),
      redirect: 'manual',
    });
    assert.equal(wrongPassword.status, 303);
    assert.match(wrongPassword.headers.get('location') ?? '', /\/login\?error=invalid$/);

    const proxiedWrongPassword = await fetch(`${baseUrl}/auth/login`, {
      method: 'POST',
      headers: {
        Host: `127.0.0.1:${new URL(baseUrl).port}`,
        Origin: 'https://ui.example.test',
        'X-Forwarded-Host': 'ui.example.test',
        'X-Forwarded-Proto': 'https',
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: new URLSearchParams({ username: 'alice', password: 'wrong-password' }),
      redirect: 'manual',
    });
    assert.equal(proxiedWrongPassword.status, 303);

    const nullOriginSameSite = await fetch(`${baseUrl}/auth/login`, {
      method: 'POST',
      headers: {
        Origin: 'null',
        Referer: `${baseUrl}/login`,
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: new URLSearchParams({ username: 'alice', password: 'wrong-password' }),
      redirect: 'manual',
    });
    assert.equal(nullOriginSameSite.status, 303);
    assert.match(nullOriginSameSite.headers.get('location') ?? '', /\/login\?error=invalid$/);

    const correctPassword = await fetch(`${baseUrl}/auth/login`, {
      method: 'POST',
      headers: {
        Origin: baseUrl,
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      body: new URLSearchParams({ username: 'alice', password: 'hunter2' }),
      redirect: 'manual',
    });
    assert.equal(correctPassword.status, 303);
    assert.equal(correctPassword.headers.get('cache-control'), 'no-store');
    assert.match(correctPassword.headers.get('set-cookie') ?? '', /underwrap_admin_session=/);
    assert.match(correctPassword.headers.get('location') ?? '', /\/$/);
  });
});

