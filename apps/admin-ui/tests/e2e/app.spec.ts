import { readFileSync } from 'node:fs';
import path from 'node:path';

import { expect, test } from '@playwright/test';

import { SESSION_COOKIE, createSession, encodeSession } from '../../lib/server/session-core.ts';

const secretValue = 'ghp_e2e_secret';
const backendToken = readFileSync(path.join(process.cwd(), 'dev-fixtures', 'corp.token'), 'utf8').trim();
const sessionKey = readFileSync(path.join(process.cwd(), 'dev-fixtures', 'session.key'));

async function resetBackend(page: import('@playwright/test').Page) {
  const response = await page.request.post('http://127.0.0.1:4011/__test/reset');
  expect(response.ok()).toBeTruthy();
}

async function seedSession(page: import('@playwright/test').Page) {
  const session = createSession('alice');
  await page.context().addCookies([
    {
      name: SESSION_COOKIE,
      value: encodeSession(session, sessionKey),
      domain: '127.0.0.1',
      path: '/',
      httpOnly: true,
      sameSite: 'Lax',
    },
  ]);
}

async function gotoResourceDetail(page: import('@playwright/test').Page) {
  await page.goto('/s/work/resources/gh/main');
  await expect(page.getByRole('heading', { name: 'Resource gh/main' })).toBeVisible();
}

async function currentCsrfToken(page: import('@playwright/test').Page) {
  const token = await page.locator('input[name="csrf_token"]').first().inputValue();
  expect(token).not.toEqual('');
  return token;
}

async function postFormAction(
  page: import('@playwright/test').Page,
  url: string,
  form: Record<string, string>,
) {
  const response = await page.context().request.post(url, {
    form,
    headers: {
      Origin: 'http://127.0.0.1:3100',
    },
  });
  expect(response.ok()).toBeTruthy();
  return response;
}

async function enableMutation(
  page: import('@playwright/test').Page,
  details: { resource: string; wrapper: string; slot: 'env' | 'files' | 'libsecret'; entry: string },
) {
  const csrfToken = await currentCsrfToken(page);
  await postFormAction(page, '/actions/resource/mutation', {
    store: 'work',
    resource: details.resource,
    wrapper: details.wrapper,
    slot: details.slot,
    entry: details.entry,
    mode: 'temporary',
    csrf_token: csrfToken,
  });
}

test.describe.configure({ mode: 'serial' });

test.beforeEach(async ({ page }) => {
  await resetBackend(page);
});

test('protected routes redirect without a valid session and pages carry CSP', async ({ page }) => {
  const loginResponse = await page.goto('/login');
  expect(loginResponse?.headers()['content-security-policy']).toContain("default-src 'self'");

  await page.goto('/s/work/resources/gh/main');
  await expect(page).toHaveURL(/\/login$/);

  await page.context().addCookies([
    {
      name: SESSION_COOKIE,
      value: 'invalid.session.cookie',
      domain: '127.0.0.1',
      path: '/',
      httpOnly: true,
      sameSite: 'Lax',
    },
  ]);
  await page.goto('/s/work/resources/gh/main');
  await expect(page).toHaveURL(/\/login$/);
});

test('resource reveal/download flow stays out of storage and uses no-store responses', async ({ page }) => {
  const consoleMessages: string[] = [];
  page.on('console', (message) => consoleMessages.push(message.text()));

  await seedSession(page);
  await page.goto('/s/work/resources/gh/main');
  const htmlBeforeReveal = await page.content();
  expect(htmlBeforeReveal).not.toContain(secretValue);
  expect(htmlBeforeReveal).not.toContain(backendToken);

  const revealPromise = page.waitForResponse((response) => response.url().endsWith('/actions/resource/reveal'));
  await page.getByRole('button', { name: 'Reveal' }).first().click();
  const revealResponse = await revealPromise;
  expect(revealResponse.headers()['cache-control']).toBe('no-store');
  await expect
    .poll(async () =>
      page.locator('textarea').evaluateAll(
        (elements, expected) =>
          elements.some((element) => (element as HTMLTextAreaElement).value === expected),
        secretValue,
      ),
    )
    .toBe(true);

  const csrfToken = await currentCsrfToken(page);
  const downloadResponse = await page.context().request.post('/actions/resource/download', {
    form: {
        store: 'work',
      resource: 'gh/main',
      secret_id: 'gh/main/files/hosts.json',
      csrf_token: csrfToken,
    },
    headers: {
      Origin: 'http://127.0.0.1:3100',
    },
  });
  expect(downloadResponse.ok()).toBeTruthy();
  expect(downloadResponse.headers()['cache-control']).toBe('no-store');
  expect(downloadResponse.headers()['content-disposition']).toContain('hosts.json');

  const storageSnapshot = await page.evaluate(() => ({
    local: { ...localStorage },
    session: { ...sessionStorage },
    cookies: document.cookie,
  }));
  expect(JSON.stringify(storageSnapshot)).not.toContain(secretValue);
  expect(JSON.stringify(storageSnapshot)).not.toContain(backendToken);
  expect(storageSnapshot.cookies).not.toContain(SESSION_COOKIE);
  expect(consoleMessages.join('\n')).not.toContain(secretValue);
  expect(consoleMessages.join('\n')).not.toContain(backendToken);
});

test('mutation, file upload/delete, wrapper mode, matrix toggle, audit filtering, and csrf rejection work end-to-end', async ({ page }) => {
  await seedSession(page);
  await gotoResourceDetail(page);

  await enableMutation(page, {
    resource: 'gh/main',
    wrapper: 'gh-authorized',
    slot: 'env',
    entry: 'GH_TOKEN',
  });
  await page.goto('/s/work/resources/gh/main?flash=mutation-enabled');
  await expect(page.getByText('Mutation window enabled for the selected wrapper and entry.')).toBeVisible();

  await postFormAction(page, '/actions/resource/write-text', {
    store: 'work',
    resource: 'gh/main',
    wrapper: 'gh-authorized',
    slot: 'env',
    entry: 'GH_TOKEN',
    value: 'ghp_replaced_secret',
    csrf_token: await currentCsrfToken(page),
  });
  await page.goto('/s/work/resources/gh/main?flash=secret-saved');
  await expect(page.getByText('Secret material stored successfully.')).toBeVisible();

  await enableMutation(page, {
    resource: 'gh/main',
    wrapper: 'gh-authorized',
    slot: 'files',
    entry: 'new.bin',
  });
  await page.goto('/s/work/resources/gh/main?flash=mutation-enabled');
  const uploadResponse = await page.context().request.post('/actions/resource/upload-file', {
    multipart: {
        store: 'work',
      resource: 'gh/main',
      wrapper: 'gh-authorized',
      entry: 'new.bin',
      csrf_token: await currentCsrfToken(page),
      file: {
        name: 'new.bin',
        mimeType: 'application/octet-stream',
        buffer: Buffer.from([9, 8, 7, 6]),
      },
    },
    headers: {
      Origin: 'http://127.0.0.1:3100',
    },
  });
  expect(uploadResponse.ok()).toBeTruthy();
  await page.goto('/s/work/resources/gh/main?flash=secret-saved');
  await expect(page.getByText('Secret material stored successfully.')).toBeVisible();
  await expect(page.getByText('new.bin')).toBeVisible();

  await postFormAction(page, '/actions/resource/delete', {
    store: 'work',
    resource: 'gh/main',
    wrapper: 'gh-authorized',
    secret_id: 'gh/main/files/new.bin',
    csrf_token: await currentCsrfToken(page),
  });
  await page.goto('/s/work/resources/gh/main?flash=secret-deleted');
  await expect(page.getByText('Stored entry deleted successfully.')).toBeVisible();
  await expect(page.getByText('new.bin')).toHaveCount(0);

  await page.goto('/s/work/wrappers');
  await expect(page.getByRole('heading', { name: 'Wrappers / work' })).toBeVisible();
  await postFormAction(page, '/actions/wrappers/set-mode', {
    store: 'work',
    wrapper: 'gh-authorized',
    mode_kind: 'unlocked_until',
    until_epoch: '1760000000',
    approval_key: '',
    reason: 'maintenance window',
    q: '',
    mode: 'any',
    resource: 'gh/main',
    host: '',
    user: '',
    matrix_wrapper: '',
    csrf_token: await currentCsrfToken(page),
  });
  await page.goto('/s/work/wrappers?flash=wrapper-mode-saved&resource=gh%2Fmain');
  await expect(page.getByText('Wrapper mode updated successfully.')).toBeVisible();
  await expect(page.locator('span', { hasText: /unlocked until/ }).first()).toBeVisible();

  await postFormAction(page, '/actions/wrappers/set-mode', {
    store: 'work',
    wrapper: 'gh-authorized',
    mode_kind: 'per_call_approval',
    until_epoch: '',
    approval_key: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey alice@example',
    reason: 'ops approval',
    q: '',
    mode: 'any',
    resource: 'gh/main',
    host: '',
    user: '',
    matrix_wrapper: '',
    csrf_token: await currentCsrfToken(page),
  });
  await page.goto('/s/work/wrappers?flash=wrapper-mode-saved&resource=gh%2Fmain');
  await expect(page.getByText('Wrapper mode updated successfully.')).toBeVisible();
  await expect(page.locator('span', { hasText: 'per-call approval' }).first()).toBeVisible();

  await postFormAction(page, '/actions/wrappers/matrix-set-cell', {
    store: 'work',
    resource: 'gh/main',
    wrapper: 'gh-authorized',
    operation: 'files.write',
    enabled: 'true',
    q: '',
    mode: 'any',
    host: '',
    user: '',
    matrix_wrapper: '',
    csrf_token: await currentCsrfToken(page),
  });
  await page.goto('/s/work/wrappers?flash=matrix-cell-enabled&resource=gh%2Fmain');
  await expect(page.getByText('Matrix allow cell enabled.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'allow' }).first()).toBeVisible();

  const downloadAuditResponse = await page.context().request.post('/actions/resource/download', {
    form: {
        store: 'work',
      resource: 'gh/main',
      secret_id: 'gh/main/files/hosts.json',
      csrf_token: await currentCsrfToken(page),
    },
    headers: {
      Origin: 'http://127.0.0.1:3100',
    },
  });
  expect(downloadAuditResponse.ok()).toBeTruthy();

  await page.goto('/s/work/audit');
  await expect(page.getByRole('heading', { name: 'Audit / work' })).toBeVisible();
  await page.selectOption('select[name="action"]', 'secret_downloaded');
  await page.getByPlaceholder('Resource').fill('gh/main');
  await page.getByRole('button', { name: 'Filter audit' }).click();
  await expect(page.locator('tbody').getByText('secret_downloaded')).toBeVisible();

  const csrfToken = await currentCsrfToken(page);
  const forbidden = await page.context().request.post('/actions/resource/reveal', {
    data: {
        store: 'work',
      secret_id: 'gh/main/env/GH_TOKEN',
      csrf_token: `${csrfToken}-wrong`,
    },
    headers: {
      'Content-Type': 'application/json',
      Origin: 'http://127.0.0.1:3100',
    },
  });
  expect(forbidden.status()).toBe(403);
});
