import test from 'node:test';
import assert from 'node:assert/strict';

import { buildSecretId, parseSecretId, validateEnvName, validateRelativeItemPath } from '../lib/server/resource-id.ts';

test('validateEnvName accepts safe names and rejects reserved ones', () => {
  assert.doesNotThrow(() => validateEnvName('GH_TOKEN'));
  assert.throws(() => validateEnvName('PATH'));
  assert.throws(() => validateEnvName('BAD-NAME'));
});

test('validateRelativeItemPath rejects traversal and absolute paths', () => {
  assert.doesNotThrow(() => validateRelativeItemPath('default/item_1'));
  assert.throws(() => validateRelativeItemPath('../etc/passwd'));
  assert.throws(() => validateRelativeItemPath('/abs/path'));
});

test('buildSecretId and parseSecretId roundtrip slot-aware ids', () => {
  const secretId = buildSecretId('gh/main', 'files', 'config.json');
  assert.equal(secretId, 'gh/main/files/config.json');
  assert.deepEqual(parseSecretId(secretId), {
    resource: 'gh/main',
    slot: 'files',
    entry: 'config.json',
  });
});
