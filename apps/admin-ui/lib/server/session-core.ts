import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';

export const SESSION_COOKIE = 'underwrap_admin_session';
export const SESSION_LIFETIME_SECONDS = 8 * 3600;

export type Session = {
  operator: string;
  issuedAtEpoch: number;
  expiresAtEpoch: number;
  csrfToken: string;
};

function nowEpoch() {
  return Math.floor(Date.now() / 1000);
}

function encodeBase64Url(input: Buffer | string) {
  return Buffer.from(input)
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/g, '');
}

function decodeBase64Url(input: string) {
  const normalized = input.replace(/-/g, '+').replace(/_/g, '/');
  return Buffer.from(normalized + '='.repeat((4 - (normalized.length % 4 || 4)) % 4), 'base64');
}

function sign(key: Buffer, body: string) {
  return createHmac('sha256', key).update(body).digest();
}

export function createSession(operator: string, issuedAtEpoch = nowEpoch()): Session {
  return {
    operator,
    issuedAtEpoch,
    expiresAtEpoch: issuedAtEpoch + SESSION_LIFETIME_SECONDS,
    csrfToken: encodeBase64Url(randomBytes(24)),
  };
}

export function encodeSession(session: Session, sessionKey: Buffer) {
  const body = JSON.stringify(session);
  const mac = sign(sessionKey, body);
  return `${encodeBase64Url(body)}.${encodeBase64Url(mac)}`;
}

export function decodeSession(value: string, sessionKey: Buffer, now = nowEpoch()): Session | null {
  const [encodedBody, encodedMac] = value.split('.');
  if (!encodedBody || !encodedMac) {
    return null;
  }

  const body = decodeBase64Url(encodedBody);
  const actualMac = decodeBase64Url(encodedMac);
  const expectedMac = sign(sessionKey, body.toString('utf8'));
  if (actualMac.length !== expectedMac.length || !timingSafeEqual(actualMac, expectedMac)) {
    return null;
  }

  const parsed = JSON.parse(body.toString('utf8')) as Partial<Session>;
  if (
    typeof parsed.operator !== 'string' ||
    typeof parsed.issuedAtEpoch !== 'number' ||
    typeof parsed.expiresAtEpoch !== 'number' ||
    typeof parsed.csrfToken !== 'string'
  ) {
    return null;
  }
  if (now >= parsed.expiresAtEpoch) {
    return null;
  }

  return parsed as Session;
}
