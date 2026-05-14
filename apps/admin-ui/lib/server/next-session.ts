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

function sessionCookieOptions() {
  return {
    httpOnly: true,
    sameSite: 'lax' as const,
    secure: process.env.NODE_ENV === 'production',
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

export async function readSession() {
  const cookieStore = await cookies();
  const raw = cookieStore.get(SESSION_COOKIE)?.value;
  if (!raw) {
    return null;
  }
  const config = await loadRuntimeConfig();
  return decodeSession(raw, config.sessionKey);
}

export async function requireSession() {
  const session = await readSession();
  if (!session) {
    redirect('/login');
  }
  return session;
}
