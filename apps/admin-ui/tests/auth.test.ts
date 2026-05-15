import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { adminBootstrapRequired, authenticateOperator, bootstrapFirstOperator } from '../lib/server/auth.ts';
import { loadRuntimeConfig } from '../lib/server/config.ts';
import { createSession, decodeSession, encodeSession } from '../lib/server/session-core.ts';
import { verifyPassword } from '../lib/server/passwords.ts';

test('verifyPassword accepts the configured operator password', async () => {
  const config = await loadRuntimeConfig();
  const ok = await verifyPassword('hunter2', config.operators.alice.passwordHash);
  assert.equal(ok, true);
});

test('verifyPassword rejects the wrong password', async () => {
  const config = await loadRuntimeConfig();
  const ok = await verifyPassword('wrong-password', config.operators.alice.passwordHash);
  assert.equal(ok, false);
});

test('authenticateOperator returns a session for valid credentials', async () => {
  const session = await authenticateOperator('alice', 'hunter2');
  assert.ok(session);
  assert.equal(session.operator, 'alice');
});

test('authenticateOperator rejects invalid credentials', async () => {
  const session = await authenticateOperator('alice', 'wrong-password');
  assert.equal(session, null);
});

test('encodeSession/decodeSession roundtrip', async () => {
  const config = await loadRuntimeConfig();
  const session = createSession('alice', 1_700_000_000);
  const encoded = encodeSession(session, config.sessionKey);
  const decoded = decodeSession(encoded, config.sessionKey, 1_700_000_001);
  assert.deepEqual(decoded, session);
});

test('decodeSession rejects expired sessions', async () => {
  const config = await loadRuntimeConfig();
  const session = createSession('alice', 1_700_000_000);
  const encoded = encodeSession(session, config.sessionKey);
  const decoded = decodeSession(encoded, config.sessionKey, session.expiresAtEpoch);
  assert.equal(decoded, null);
});

test('bootstrapFirstOperator creates the first operator in local state and enables login', async () => {
  const tempDir = await mkdtemp(path.join(os.tmpdir(), 'underwrap-admin-ui-auth-'));
  const stateDir = await mkdtemp(path.join(os.tmpdir(), 'underwrap-admin-ui-state-'));
  const configPath = path.join(tempDir, 'admin-ui.yaml');
  const root = process.cwd();
  await writeFile(
    configPath,
    [
      `session_key_file: ${path.join(root, 'dev-fixtures', 'session.key')}`,
      '',
      'backend:',
      '  label: corp',
      `  admin_url: http://127.0.0.1:19443`,
      `  token_file: ${path.join(root, 'dev-fixtures', 'corp.token')}`,
      '',
    ].join('\n'),
  );

  const previousConfig = process.env.UNDERWRAP_ADMIN_UI_CONFIG;
  const previousStateDir = process.env.UNDERWRAP_ADMIN_UI_STATE_DIR;
  try {
    process.env.UNDERWRAP_ADMIN_UI_CONFIG = configPath;
    process.env.UNDERWRAP_ADMIN_UI_STATE_DIR = stateDir;

    assert.equal(await adminBootstrapRequired(), true);
    const session = await bootstrapFirstOperator('robert', 'test-password-123');
    assert.equal(session.operator, 'robert');
    assert.equal(await adminBootstrapRequired(), false);

    const stored = await readFile(path.join(stateDir, 'operators.json'), 'utf8');
    assert.match(stored, /\$argon2id\$/);

    const loginSession = await authenticateOperator('robert', 'test-password-123');
    assert.ok(loginSession);
    assert.equal(loginSession?.operator, 'robert');
    assert.equal(await authenticateOperator('robert', 'wrong-password'), null);
  } finally {
    if (previousConfig === undefined) {
      delete process.env.UNDERWRAP_ADMIN_UI_CONFIG;
    } else {
      process.env.UNDERWRAP_ADMIN_UI_CONFIG = previousConfig;
    }
    if (previousStateDir === undefined) {
      delete process.env.UNDERWRAP_ADMIN_UI_STATE_DIR;
    } else {
      process.env.UNDERWRAP_ADMIN_UI_STATE_DIR = previousStateDir;
    }
  }
});
