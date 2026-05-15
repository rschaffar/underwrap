import { AuditFilterForm } from '../../../../components/audit-filter-form';
import { Card } from '../../../../components/card';
import { Shell } from '../../../../components/shell';
import { buildAuditQuery, auditActionTone, parseEpochSearchParam } from '../../../../lib/server/audit-filters.ts';
import { requireBackendClient } from '../../../../lib/server/navigation.ts';

function formatTimestamp(epoch: number) {
  return new Date(epoch * 1000).toISOString().replace('T', ' ').replace('.000Z', 'Z');
}

export default async function AuditPage({
  params,
  searchParams,
}: {
  params: Promise<{ store: string }>;
  searchParams: Promise<{
    operator?: string;
    wrapper?: string;
    resource?: string;
    action?: string;
    from_epoch?: string;
    to_epoch?: string;
  }>;
}) {
  const [{ store }, filters] = await Promise.all([params, searchParams]);
  const client = await requireBackendClient();
  const query = buildAuditQuery(filters);

  let response;
  try {
    response = await client.queryAudit(store, query);
  } catch {
    return (
      <Shell
        title={`Audit / ${store}`}
        description="The audit feed could not be loaded right now."
        store={store}
        section="audit"
      >
        <Card title="Audit console unavailable" eyebrow="Audit console">
          <p className="text-sm text-[var(--muted)]">Check backend reachability and audit endpoint health, then try again.</p>
        </Card>
      </Shell>
    );
  }

  return (
    <Shell
      title={`Audit / ${store}`}
      description="Read-only, filterable backend audit history with explicit reveal/download visibility for the new operator console."
      store={store}
      section="audit"
    >
      <div className="grid gap-6">
        <Card title="Filters" eyebrow="Read-only query">
          <AuditFilterForm
            defaultValues={{
              operator: filters.operator,
              wrapper: filters.wrapper,
              resource: filters.resource,
              action: filters.action,
              from_epoch: parseEpochSearchParam(filters.from_epoch),
              to_epoch: parseEpochSearchParam(filters.to_epoch),
            }}
          />
        </Card>

        <Card title="Audit history" eyebrow="Live data">
          <div className="mb-4 flex flex-wrap gap-4 text-sm text-[var(--muted)]">
            <div>Results: <span className="text-white">{response.total}</span></div>
            <div>Store: <span className="text-white">{store}</span></div>
          </div>

          {response.items.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
              No audit events matched the current filter.
            </div>
          ) : (
            <div className="overflow-hidden rounded-2xl border border-white/10">
              <table className="min-w-full divide-y divide-white/10 text-left text-sm">
                <thead className="bg-black/30 text-[var(--muted)]">
                  <tr>
                    <th className="px-4 py-3 font-medium">Timestamp</th>
                    <th className="px-4 py-3 font-medium">Action</th>
                    <th className="px-4 py-3 font-medium">Operator</th>
                    <th className="px-4 py-3 font-medium">Wrapper</th>
                    <th className="px-4 py-3 font-medium">Resource</th>
                    <th className="px-4 py-3 font-medium">Detail</th>
                    <th className="px-4 py-3 font-medium">Result</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/10 bg-black/10">
                  {response.items.map((row, index) => (
                    <tr key={`${row.ts_epoch}-${row.action}-${index}`}>
                      <td className="px-4 py-4 text-white">{formatTimestamp(row.ts_epoch)}</td>
                      <td className={`px-4 py-4 ${auditActionTone(row.action)}`}>{row.action}</td>
                      <td className="px-4 py-4 text-[var(--muted)]">{row.operator}</td>
                      <td className="px-4 py-4 text-[var(--muted)]">{row.wrapper ?? '—'}</td>
                      <td className="px-4 py-4 text-[var(--muted)]">{row.resource ?? '—'}</td>
                      <td className="px-4 py-4 text-[var(--muted)]">
                        {row.operation ?? row.secret_id ?? '—'}
                      </td>
                      <td className={row.result === 'success' ? 'px-4 py-4 text-emerald-300' : 'px-4 py-4 text-rose-300'}>
                        {row.result}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </Shell>
  );
}
