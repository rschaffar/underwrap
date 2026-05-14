import test from 'node:test';
import assert from 'node:assert/strict';

import { csrfTokenMatches, requestHasAllowedOrigin } from '../lib/server/csrf.ts';

test('csrfTokenMatches accepts equal tokens and rejects different ones', () => {
  assert.equal(csrfTokenMatches('abc', 'abc'), true);
  assert.equal(csrfTokenMatches('abc', 'abd'), false);
  assert.equal(csrfTokenMatches('abc', 'abcd'), false);
});

test('requestHasAllowedOrigin requires same-origin POSTs', () => {
  const ok = new Request('http://127.0.0.1:3000/logout', {
    method: 'POST',
    headers: {
      origin: 'http://127.0.0.1:3000',
      host: '127.0.0.1:3000',
    },
  });
  const bad = new Request('http://127.0.0.1:3000/logout', {
    method: 'POST',
    headers: {
      origin: 'https://evil.example',
      host: '127.0.0.1:3000',
    },
  });
  assert.equal(requestHasAllowedOrigin(ok), true);
  assert.equal(requestHasAllowedOrigin(bad), false);
});

