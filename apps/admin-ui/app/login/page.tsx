import { redirect } from 'next/navigation';

import { Card } from '../../components/card';
import { readSession } from '../../lib/auth-session.ts';

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ error?: string }>;
}) {
  const [session, resolvedSearchParams] = await Promise.all([readSession(), searchParams]);
  if (session) {
    redirect('/');
  }

  const invalidCredentials = resolvedSearchParams.error === 'invalid';

  return (
    <div className="flex min-h-screen items-center justify-center px-4 py-8">
      <div className="w-full max-w-md">
        <Card title="Operator sign-in" eyebrow="Username + Argon2 password">
          <form className="space-y-4" method="post" action="/auth/login">
            <label className="block">
              <div className="mb-2 text-sm font-medium text-white">Username</div>
              <input
                type="text"
                name="username"
                placeholder="alice"
                className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
              />
            </label>
            <label className="block">
              <div className="mb-2 text-sm font-medium text-white">Password</div>
              <input
                type="password"
                name="password"
                placeholder="••••••••"
                className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
              />
            </label>
            {invalidCredentials ? (
              <div className="rounded-2xl border border-rose-400/30 bg-rose-400/10 px-4 py-3 text-sm text-rose-100">
                Invalid credentials.
              </div>
            ) : null}
            <button
              type="submit"
              className="w-full rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 font-medium text-cyan-100 transition hover:bg-cyan-400/15"
            >
              Sign in
            </button>
          </form>
          <p className="mt-4 text-sm text-[var(--muted)]">
            Local scaffold fixture credentials: <span className="text-white">alice</span> / <span className="text-white">hunter2</span>
          </p>
        </Card>
      </div>
    </div>
  );
}
