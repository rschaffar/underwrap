import { timingSafeEqual } from 'node:crypto';

function normalizeOriginCandidate(value: string | null) {
  if (!value) {
    return null;
  }
  try {
    return new URL(value).origin;
  } catch {
    return null;
  }
}

export function csrfTokenMatches(expected: string, given: string) {
  const expectedBuffer = Buffer.from(expected);
  const givenBuffer = Buffer.from(given);
  return expectedBuffer.length === givenBuffer.length && timingSafeEqual(expectedBuffer, givenBuffer);
}

export function expectedOriginForRequest(request: Request) {
  const host = request.headers.get('x-forwarded-host') ?? request.headers.get('host');
  if (!host) {
    return null;
  }
  const proto = request.headers.get('x-forwarded-proto') ?? 'http';
  return normalizeOriginCandidate(`${proto}://${host}`);
}

export function requestHasAllowedOrigin(request: Request) {
  const requestOrigin = normalizeOriginCandidate(request.headers.get('origin'));
  const expectedOrigin = expectedOriginForRequest(request);
  return requestOrigin !== null && expectedOrigin !== null && requestOrigin === expectedOrigin;
}
