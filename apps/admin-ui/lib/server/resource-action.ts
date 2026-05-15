import { NextResponse } from 'next/server';

import { csrfTokenMatches, requestHasAllowedOrigin } from './csrf.ts';
import { readSessionFromCookieHeader } from './next-session.ts';

export async function requireActionSession(request: Request, csrfToken: string) {
  if (!requestHasAllowedOrigin(request)) {
    return { error: new Response('forbidden', { status: 403 }) } as const;
  }

  const session = await readSessionFromCookieHeader(request.headers.get('cookie'));
  if (!session || !csrfTokenMatches(session.csrfToken, csrfToken)) {
    return { error: new Response('forbidden', { status: 403 }) } as const;
  }

  return { session } as const;
}

export function resourceDetailHref(store: string, resource: string, flash?: string) {
  const base = `/s/${encodeURIComponent(store)}/resources/${resource
    .split('/')
    .map(encodeURIComponent)
    .join('/')}`;
  return flash ? `${base}?flash=${encodeURIComponent(flash)}` : base;
}

export function redirectToResourceDetail(request: Request, store: string, resource: string, flash?: string) {
  return NextResponse.redirect(new URL(resourceDetailHref(store, resource, flash), request.url), 303);
}
