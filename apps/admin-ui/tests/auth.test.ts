import test from 'node:test';
import assert from 'node:assert/strict';

import { authenticateOperator } from '../lib/server/auth.ts';
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
