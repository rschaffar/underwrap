import { NextResponse } from 'next/server';

import { authenticateOperator } from '../../../lib/server/auth.ts';
import { loadRuntimeConfig } from '../../../lib/server/config.ts';
import { setSessionCookie } from '../../../lib/auth-session';

export async function POST(request: Request) {
  const formData = await request.formData();
  const username = String(formData.get('username') ?? '');
  const password = String(formData.get('password') ?? '');

  const session = await authenticateOperator(username, password);
  if (!session) {
    return NextResponse.redirect(new URL('/login?error=invalid', request.url), 303);
  }

  const config = await loadRuntimeConfig();
  const response = NextResponse.redirect(new URL('/', request.url), 303);
  setSessionCookie(response, config.sessionKey, session);
  return response;
}
