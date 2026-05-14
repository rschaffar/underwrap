import { Card } from '../../components/card';

export default function LoginPage() {
  return (
    <div className="flex min-h-screen items-center justify-center px-4 py-8">
      <div className="w-full max-w-md">
        <Card title="Operator sign-in" eyebrow="Placeholder route">
          <form className="space-y-4">
            <label className="block">
              <div className="mb-2 text-sm font-medium text-white">Username</div>
              <input
                type="text"
                placeholder="alice"
                className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
              />
            </label>
            <label className="block">
              <div className="mb-2 text-sm font-medium text-white">Password</div>
              <input
                type="password"
                placeholder="••••••••"
                className="w-full rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white outline-none ring-0 placeholder:text-[var(--muted)] focus:border-cyan-400/40"
              />
            </label>
            <button
              type="button"
              className="w-full rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 font-medium text-cyan-100 transition hover:bg-cyan-400/15"
            >
              Sign in (auth lands in ticket 040)
            </button>
          </form>
        </Card>
      </div>
    </div>
  );
}
