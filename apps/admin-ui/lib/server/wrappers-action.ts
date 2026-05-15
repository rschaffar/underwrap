import { NextResponse } from 'next/server';

export type WrappersPageQuery = {
  q?: string;
  mode?: string;
  resource?: string;
  host?: string;
  user?: string;
  matrix_wrapper?: string;
  flash?: string;
};

export function wrappersHref(store: string, query: WrappersPageQuery = {}) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (typeof value === 'string' && value.trim()) {
      params.set(key, value.trim());
    }
  }
  const qs = params.toString();
  const base = `/s/${encodeURIComponent(store)}/wrappers`;
  return qs ? `${base}?${qs}` : base;
}

export function redirectToWrappers(request: Request, store: string, query: WrappersPageQuery = {}) {
  return NextResponse.redirect(new URL(wrappersHref(store, query), request.url), 303);
}
