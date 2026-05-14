import test from 'node:test';
import assert from 'node:assert/strict';

import { csrfTokenMatches, requestHasAllowedOrigin } from '../lib/server/csrf.ts';

test('csrfTokenMatches accepts equal tokens and rejects different ones', () => {
  assert.equal(csrfTokenMatches('abc', 'abc'), true);
  assert.equal(csrfTokenMatches('abc', 'abd'), false);
  assert.equal(csrfTokenMatches('abc', 'abcd'), false);
});

test('requestHasAllowedOrigin accepts same-origin Origin/Referer/fetch-metadata and rejects cross-origin requests', () => {
  const ok = new Request('http://127.0.0.1:3000/logout', {
    method: 'POST',
    headers: {
      origin: 'http://127.0.0.1:3000',
      host: '127.0.0.1:3000',
    },
  });
  const refererOk = new Request('http://127.0.0.1:3000/logout', {
    method: 'POST',
    headers: {
      referer: 'http://127.0.0.1:3000/t/corp/s/work/resources/gh/main',
      host: '127.0.0.1:3000',
    },
  });
  const fetchMetadataOk = new Request('http://127.0.0.1:3000/logout', {
    method: 'POST',
    headers: {
      'sec-fetch-site': 'same-origin',
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
  assert.equal(requestHasAllowedOrigin(refererOk), true);
  assert.equal(requestHasAllowedOrigin(fetchMetadataOk), true);
  assert.equal(requestHasAllowedOrigin(bad), false);
});

