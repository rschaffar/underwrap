import Link from 'next/link';
import { ReactNode } from 'react';

import { NavSection } from '../lib/mock-data';
import { requireSession } from '../lib/auth-session.ts';
import { loadNavigation } from '../lib/server/navigation.ts';

type ShellProps = {
  children: ReactNode;
  title: string;
  description: string;
  section?: NavSection;
  target?: string;
  store?: string;
};

function sectionHref(target: string, store: string | undefined, section: NavSection) {
  if (!store || section === 'overview') {
    return `/t/${target}`;
  }
  switch (section) {
    case 'resources':
      return `/t/${target}/s/${store}/resources`;
    case 'wrappers':
      return `/t/${target}/s/${store}/wrappers`;
    case 'audit':
      return `/t/${target}/s/${store}/audit`;
  }
}

function StatusPill({ reachable }: { reachable: boolean }) {
  return (
    <span
      className={reachable
        ? 'inline-flex items-center rounded-full border border-emerald-500/40 bg-emerald-500/10 px-2.5 py-1 text-xs font-semibold text-emerald-300'
        : 'inline-flex items-center rounded-full border border-amber-500/40 bg-amber-500/10 px-2.5 py-1 text-xs font-semibold text-amber-300'}
    >
      {reachable ? 'Reachable' : 'Degraded'}
    </span>
  );
}

export async function Shell({ children, title, description, section, target, store }: ShellProps) {
  const [session, navigation] = await Promise.all([requireSession(), loadNavigation(target)]);
  const targets = navigation.targets;
  const selectedTarget = navigation.activeTarget;
  const stores = selectedTarget?.stores ?? [];

  return (
    <div className="min-h-screen bg-[var(--bg)] text-[var(--fg)]">
      <div className="mx-auto flex min-h-screen max-w-7xl flex-col px-4 py-4 sm:px-6 lg:px-8">
        <header className="sticky top-0 z-20 mb-6 rounded-3xl border border-white/10 bg-[var(--surface-0)]/95 p-4 shadow-2xl shadow-black/30 backdrop-blur">
          <div className="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
            <div>
              <div className="text-xs font-semibold uppercase tracking-[0.3em] text-[var(--muted)]">
                underwrap admin console
              </div>
              <div className="mt-2 flex items-center gap-3">
                <h1 className="text-2xl font-semibold tracking-tight text-white">{title}</h1>
                {selectedTarget ? <StatusPill reachable={selectedTarget.status === 'reachable'} /> : null}
              </div>
              <p className="mt-2 max-w-3xl text-sm text-[var(--muted)]">{description}</p>
            </div>
            <div className="flex items-start gap-3 rounded-2xl border border-cyan-400/20 bg-cyan-400/5 px-4 py-3 text-sm text-cyan-100 shadow-lg shadow-cyan-950/30">
              <div>
                <div className="font-medium">Signed in as {session.operator}</div>
                <div className="mt-1 text-cyan-100/75">Session handling is active; CSRF protections land in the next ticket.</div>
              </div>
              <form method="post" action="/logout" className="ml-auto">
                <input type="hidden" name="csrf_token" value={session.csrfToken} />
                <button type="submit" className="rounded-xl border border-cyan-300/30 bg-cyan-300/10 px-3 py-2 text-sm font-medium text-cyan-50 transition hover:bg-cyan-300/15">
                  Sign out
                </button>
              </form>
            </div>
          </div>

          <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
            <div className="grid gap-4 md:grid-cols-2">
              <div className="rounded-2xl border border-white/8 bg-black/20 p-3">
                <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.25em] text-[var(--muted)]">Target</div>
                <div className="flex flex-wrap gap-2">
                  {targets.map((entry) => {
                    const active = entry.slug === selectedTarget?.slug;
                    return (
                      <Link
                        key={entry.slug}
                        href={`/t/${entry.slug}`}
                        className={active
                          ? 'rounded-xl border border-cyan-400/50 bg-cyan-400/15 px-3 py-2 text-sm font-medium text-cyan-100'
                          : 'rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-[var(--fg)] transition hover:border-cyan-400/30 hover:bg-cyan-400/10 hover:text-cyan-100'}
                      >
                        {entry.label}
                      </Link>
                    );
                  })}
                </div>
              </div>

              <div className="rounded-2xl border border-white/8 bg-black/20 p-3">
                <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.25em] text-[var(--muted)]">Store</div>
                <div className="flex flex-wrap gap-2">
                  {stores.length > 0 ? (
                    stores.map((entry) => {
                      const active = entry === store;
                      return (
                        <Link
                          key={entry}
                          href={`/t/${selectedTarget?.slug}/s/${entry}/resources`}
                          className={active
                            ? 'rounded-xl border border-fuchsia-400/50 bg-fuchsia-400/15 px-3 py-2 text-sm font-medium text-fuchsia-100'
                            : 'rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-[var(--fg)] transition hover:border-fuchsia-400/30 hover:bg-fuchsia-400/10 hover:text-fuchsia-100'}
                        >
                          {entry}
                        </Link>
                      );
                    })
                  ) : (
                    <div className="rounded-xl border border-dashed border-white/10 px-3 py-2 text-sm text-[var(--muted)]">
                      Select a target to see its stores.
                    </div>
                  )}
                </div>
              </div>
            </div>

            <nav className="flex flex-wrap gap-2">
              {(['overview', 'resources', 'wrappers', 'audit'] as NavSection[]).map((entry) => {
                const active = entry === section;
                return (
                  <Link
                    key={entry}
                    href={sectionHref(selectedTarget?.slug ?? targets[0]?.slug ?? '', store, entry)}
                    className={active
                      ? 'rounded-xl border border-white/20 bg-white/12 px-4 py-2 text-sm font-medium text-white shadow-lg shadow-black/20'
                      : 'rounded-xl border border-white/8 bg-white/5 px-4 py-2 text-sm text-[var(--muted)] transition hover:border-white/15 hover:bg-white/8 hover:text-white'}
                  >
                    {entry[0].toUpperCase() + entry.slice(1)}
                  </Link>
                );
              })}
            </nav>
          </div>
        </header>

        <main className="flex-1">{children}</main>
      </div>
    </div>
  );
}
