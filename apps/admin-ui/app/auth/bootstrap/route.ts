import { NextResponse } from 'next/server';

import { setSessionCookie } from '../../../lib/auth-session.ts';
import { bootstrapFirstOperator } from '../../../lib/server/auth.ts';
import { loadRuntimeConfig } from '../../../lib/server/config.ts';
import { requestHasAllowedOrigin } from '../../../lib/server/csrf.ts';

export async function POST(request: Request) {
  if (!requestHasAllowedOrigin(request)) {
    return new Response('forbidden', { status: 403 });
  }

  const formData = await request.formData();
  const username = String(formData.get('username') ?? '');
  const password = String(formData.get('password') ?? '');
  const passwordConfirm = String(formData.get('password_confirm') ?? '');

  if (!username.trim()) {
    return NextResponse.redirect(new URL('/login?bootstrap_error=username', request.url), 303);
  }
  if (!password) {
    return NextResponse.redirect(new URL('/login?bootstrap_error=password', request.url), 303);
  }
  if (password !== passwordConfirm) {
    return NextResponse.redirect(new URL('/login?bootstrap_error=mismatch', request.url), 303);
  }

  try {
    const session = await bootstrapFirstOperator(username, password);
    const config = await loadRuntimeConfig();
    const response = NextResponse.redirect(new URL('/', request.url), 303);
    setSessionCookie(response, config.sessionKey, session);
    return response;
  } catch (error: unknown) {
    const message = String(error);
    const code = message.includes('already complete') ? 'complete' : 'failed';
    return NextResponse.redirect(new URL(`/login?bootstrap_error=${code}`, request.url), 303);
  }
}
