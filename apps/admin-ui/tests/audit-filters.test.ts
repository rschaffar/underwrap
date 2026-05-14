import test from 'node:test';
import assert from 'node:assert/strict';

import { auditActionTone, buildAuditQuery, epochToDateTimeLocal, parseEpochSearchParam } from '../lib/server/audit-filters.ts';

test('parseEpochSearchParam accepts positive integers and rejects junk', () => {
  assert.equal(parseEpochSearchParam('1760000000'), 1760000000);
  assert.equal(parseEpochSearchParam('0'), null);
  assert.equal(parseEpochSearchParam('nope'), null);
});

test('epochToDateTimeLocal returns a datetime-local formatted string', () => {
  const value = epochToDateTimeLocal(1760000000);
  assert.match(value, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);
});

test('buildAuditQuery trims text filters and normalizes action/epoch values', () => {
  assert.deepEqual(
    buildAuditQuery({
      operator: ' alice ',
      wrapper: ' gh-authorized ',
      resource: ' gh/main ',
      action: 'secret_revealed',
      from_epoch: '1760000000',
      to_epoch: '1760003600',
    }),
    {
      operator: 'alice',
      wrapper: 'gh-authorized',
      resource: 'gh/main',
      action: 'secret_revealed',
      from_epoch: 1760000000,
      to_epoch: 1760003600,
    },
  );

  assert.deepEqual(buildAuditQuery({ action: 'any' }), {
    operator: undefined,
    wrapper: undefined,
    resource: undefined,
    action: undefined,
    from_epoch: null,
    to_epoch: null,
  });
});

test('auditActionTone highlights reveal/download actions distinctly', () => {
  assert.equal(auditActionTone('secret_revealed'), 'text-cyan-100');
  assert.equal(auditActionTone('secret_downloaded'), 'text-cyan-100');
  assert.equal(auditActionTone('wrapper_mode_updated'), 'text-fuchsia-200');
});
