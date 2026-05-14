import { NextResponse } from 'next/server';

import { clearSessionCookie } from '../../lib/auth-session.ts';
import { csrfTokenMatches, requestHasAllowedOrigin } from '../../lib/server/csrf.ts';
import { readSessionFromCookieHeader } from '../../lib/server/next-session.ts';

export async function POST(request: Request) {
  if (!requestHasAllowedOrigin(request)) {
    return new Response('forbidden', { status: 403 });
  }

  const session = await readSessionFromCookieHeader(request.headers.get('cookie'));
  const formData = await request.formData();
  const csrfToken = String(formData.get('csrf_token') ?? '');
  if (!session || !csrfTokenMatches(session.csrfToken, csrfToken)) {
    return new Response('forbidden', { status: 403 });
  }

  const response = NextResponse.redirect(new URL('/login', request.url), 303);
  clearSessionCookie(response);
  return response;
}
