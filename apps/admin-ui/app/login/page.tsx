import { redirect } from 'next/navigation';

import { Card } from '../../components/card';
import { readSession } from '../../lib/auth-session.ts';
import { adminBootstrapRequired } from '../../lib/server/auth.ts';
import { loadRuntimeConfig } from '../../lib/server/config.ts';

function bootstrapErrorMessage(code: string | undefined) {
  switch (code) {
    case 'username':
      return 'Choose an admin username.';
    case 'password':
      return 'Choose a password.';
    case 'mismatch':
      return 'Password confirmation does not match.';
    case 'complete':
      return 'Bootstrap is already complete. Sign in with an existing operator account.';
    case 'failed':
      return 'Bootstrap failed.';
    default:
      return null;
  }
}

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ error?: string; bootstrap_error?: string }>;
}) {
  const [session, resolvedSearchParams, needsBootstrap, config] = await Promise.all([
    readSession(),
    searchParams,
    adminBootstrapRequired(),
    loadRuntimeConfig(),
  ]);
  if (session) {
    redirect('/');
  }

  const invalidCredentials = resolvedSearchParams.error === 'invalid';
  const bootstrapError = bootstrapErrorMessage(resolvedSearchParams.bootstrap_error);
  const showFixtureHint = !needsBootstrap && config.sourcePath.includes('dev-fixtures') && Boolean(config.operators.alice);

  return (
    <div className="flex min-h-screen items-center justify-center px-4 py-8">
      <div className="w-full max-w-md">
        <Card
          title={needsBootstrap ? 'Initialize admin UI' : 'Operator sign-in'}
          eyebrow={needsBootstrap ? 'First-use bootstrap' : 'Username + Argon2 password'}
        >
          {needsBootstrap ? (
            <>
              <form className="space-y-4" method="post" action="/auth/bootstrap">
                <label className="block">
                  <div className="mb-2 text-sm font-medium text-white">Admin username</div>
                  <input
                    type="text"
                    name="username"
                    placeholder="robert"
                    className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
                  />
                </label>
                <label className="block">
                  <div className="mb-2 text-sm font-medium text-white">Password</div>
                  <input
                    type="password"
                    name="password"
                    placeholder="Choose a password"
                    className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
                  />
                </label>
                <label className="block">
                  <div className="mb-2 text-sm font-medium text-white">Confirm password</div>
                  <input
                    type="password"
                    name="password_confirm"
                    placeholder="Repeat the password"
                    className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
                  />
                </label>
                {bootstrapError ? (
                  <div className="rounded-2xl border border-rose-400/30 bg-rose-400/10 px-4 py-3 text-sm text-rose-100">
                    {bootstrapError}
                  </div>
                ) : null}
                <button
                  type="submit"
                  className="w-full rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 font-medium text-cyan-100 transition hover:bg-cyan-400/15"
                >
                  Create first admin
                </button>
              </form>
              <p className="mt-4 text-sm text-[var(--muted)]">
                No operator account is configured yet. The first person to access this page must choose the initial admin username and password.
              </p>
            </>
          ) : (
            <>
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
              {showFixtureHint ? (
                <p className="mt-4 text-sm text-[var(--muted)]">
                  Local scaffold fixture credentials: <span className="text-white">alice</span> / <span className="text-white">hunter2</span>
                </p>
              ) : null}
            </>
          )}
        </Card>
      </div>
    </div>
  );
}
