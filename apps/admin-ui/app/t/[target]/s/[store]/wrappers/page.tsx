import { Card } from '../../../../../../components/card';
import { Shell } from '../../../../../../components/shell';

const wrappers = [
  { wrapper: 'gh-authorized', mode: 'per-call approval', reason: 'ops approval required', resources: 2 },
  { wrapper: 'aws-prod', mode: 'locked', reason: 'change freeze', resources: 1 },
  { wrapper: 'jira-readonly', mode: 'unlocked until 2026-05-15 18:00 UTC', reason: 'maintenance window', resources: 3 },
];

export default async function WrappersPage({
  params,
}: {
  params: Promise<{ target: string; store: string }>;
}) {
  const { target, store } = await params;

  return (
    <Shell
      title={`Wrappers / ${store}`}
      description="Filterable wrapper console scaffold with strong visual mode states and room for later access-matrix integration."
      target={target}
      store={store}
      section="wrappers"
    >
      <Card title="Wrapper console" eyebrow="Placeholder data">
        <div className="overflow-hidden rounded-2xl border border-white/10">
          <table className="min-w-full divide-y divide-white/10 text-left text-sm">
            <thead className="bg-black/30 text-[var(--muted)]">
              <tr>
                <th className="px-4 py-3 font-medium">Wrapper</th>
                <th className="px-4 py-3 font-medium">Mode</th>
                <th className="px-4 py-3 font-medium">Reason</th>
                <th className="px-4 py-3 font-medium">Resources</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10 bg-black/10">
              {wrappers.map((wrapper) => (
                <tr key={wrapper.wrapper}>
                  <td className="px-4 py-4 text-white">{wrapper.wrapper}</td>
                  <td className="px-4 py-4">
                    <span className={wrapper.mode.startsWith('locked') ? 'rounded-full border border-rose-400/30 bg-rose-400/10 px-2.5 py-1 text-xs font-medium text-rose-200' : wrapper.mode.startsWith('per-call') ? 'rounded-full border border-amber-400/30 bg-amber-400/10 px-2.5 py-1 text-xs font-medium text-amber-200' : 'rounded-full border border-emerald-400/30 bg-emerald-400/10 px-2.5 py-1 text-xs font-medium text-emerald-200'}>
                      {wrapper.mode}
                    </span>
                  </td>
                  <td className="px-4 py-4 text-[var(--muted)]">{wrapper.reason}</td>
                  <td className="px-4 py-4 text-white">{wrapper.resources}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </Shell>
  );
}
