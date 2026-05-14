import { Card } from '@/components/card';
import { Shell } from '@/components/shell';

const auditRows = [
  { ts: '2026-05-14 10:14:22Z', action: 'secret_revealed', operator: 'alice', resource: 'gh/main', result: 'allowed' },
  { ts: '2026-05-14 10:16:40Z', action: 'mutation_enabled', operator: 'alice', resource: 'aws/prod', result: 'allowed' },
  { ts: '2026-05-14 10:20:03Z', action: 'secret_downloaded', operator: 'bob', resource: 'gws/castlabs', result: 'allowed' },
];

export default async function AuditPage({
  params,
}: {
  params: Promise<{ target: string; store: string }>;
}) {
  const { target, store } = await params;

  return (
    <Shell
      title={`Audit / ${store}`}
      description="Read-only audit console scaffold with the dense, filterable presentation the later audit tickets will flesh out."
      target={target}
      store={store}
      section="audit"
    >
      <div className="grid gap-6">
        <Card title="Filters" eyebrow="Placeholder controls">
          <div className="grid gap-3 lg:grid-cols-4">
            {['Operator', 'Wrapper', 'Resource', 'Action'].map((field) => (
              <div key={field} className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-sm text-[var(--muted)]">
                {field} filter placeholder
              </div>
            ))}
          </div>
        </Card>

        <Card title="Audit history" eyebrow="Placeholder data">
          <div className="overflow-hidden rounded-2xl border border-white/10">
            <table className="min-w-full divide-y divide-white/10 text-left text-sm">
              <thead className="bg-black/30 text-[var(--muted)]">
                <tr>
                  <th className="px-4 py-3 font-medium">Timestamp</th>
                  <th className="px-4 py-3 font-medium">Action</th>
                  <th className="px-4 py-3 font-medium">Operator</th>
                  <th className="px-4 py-3 font-medium">Resource</th>
                  <th className="px-4 py-3 font-medium">Result</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/10 bg-black/10">
                {auditRows.map((row) => (
                  <tr key={`${row.ts}-${row.action}`}>
                    <td className="px-4 py-4 text-white">{row.ts}</td>
                    <td className="px-4 py-4 text-cyan-100">{row.action}</td>
                    <td className="px-4 py-4 text-[var(--muted)]">{row.operator}</td>
                    <td className="px-4 py-4 text-[var(--muted)]">{row.resource}</td>
                    <td className="px-4 py-4 text-emerald-300">{row.result}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
    </Shell>
  );
}
