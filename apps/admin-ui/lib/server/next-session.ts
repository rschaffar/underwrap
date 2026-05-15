import { cookies } from 'next/headers';
import { redirect } from 'next/navigation';
import type { NextResponse } from 'next/server';

import { loadRuntimeConfig } from './config.ts';
import {
  SESSION_COOKIE,
  SESSION_LIFETIME_SECONDS,
  type Session,
  createSession,
  decodeSession,
  encodeSession,
} from './session-core.ts';

export { SESSION_COOKIE, SESSION_LIFETIME_SECONDS, createSession, decodeSession, encodeSession };
export type { Session };

function secureCookiesEnabled() {
  const override = process.env.UNDERWRAP_ADMIN_UI_SECURE_COOKIES;
  if (override === 'true') {
    return true;
  }
  if (override === 'false') {
    return false;
  }
  return process.env.NODE_ENV === 'production';
}

function sessionCookieOptions() {
  return {
    httpOnly: true,
    sameSite: 'lax' as const,
    secure: secureCookiesEnabled(),
    path: '/',
    maxAge: SESSION_LIFETIME_SECONDS,
  };
}

export function setSessionCookie(response: NextResponse, sessionKey: Buffer, session: Session) {
  response.cookies.set(SESSION_COOKIE, encodeSession(session, sessionKey), sessionCookieOptions());
}

export function clearSessionCookie(response: NextResponse) {
  response.cookies.set(SESSION_COOKIE, '', {
    ...sessionCookieOptions(),
    maxAge: 0,
    expires: new Date(0),
  });
}

function cookieValueFromHeader(cookieHeader: string | null, name: string) {
  if (!cookieHeader) {
    return null;
  }
  for (const part of cookieHeader.split(';')) {
    const [cookieName, ...valueParts] = part.trim().split('=');
    if (cookieName === name) {
      return valueParts.join('=');
    }
  }
  return null;
}

export async function readSessionFromCookieHeader(cookieHeader: string | null) {
  const raw = cookieValueFromHeader(cookieHeader, SESSION_COOKIE);
  if (!raw) {
    return null;
  }
  const config = await loadRuntimeConfig();
  return decodeSession(raw, config.sessionKey);
}

export async function readSession() {
  const cookieStore = await cookies();
  return readSessionFromCookieHeader(cookieStore.get(SESSION_COOKIE)?.name ? `${SESSION_COOKIE}=${cookieStore.get(SESSION_COOKIE)?.value}` : null);
}

export async function requireSession() {
  const session = await readSession();
  if (!session) {
    redirect('/login');
  }
  return session;
}
