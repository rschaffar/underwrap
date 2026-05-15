import Link from 'next/link';

import { Card } from '../components/card';
import { MetricGrid } from '../components/metric-grid';
import { Shell } from '../components/shell';
import { getConfiguredBackendClient } from '../lib/server/backend-client.ts';

export default async function HomePage() {
  const client = await getConfiguredBackendClient();

  try {
    const overview = await client.getOverview();
    const resourceCount = overview.stores.reduce((sum, store) => sum + store.resource_count, 0);
    const recentAuditEvents = overview.stores.reduce((sum, store) => sum + store.recent_audit_events, 0);

    return (
      <Shell
        title="Operator dashboard"
        description="One admin console fronts one backend. Jump directly into its stores, resource inventory, wrapper controls, and audit history."
        section="overview"
      >
        <div className="grid gap-6 xl:grid-cols-[minmax(0,1.25fr)_minmax(320px,0.75fr)]">
          <div className="grid gap-6">
            <Card title="Configured backend" eyebrow="Live overview">
              <MetricGrid
                metrics={[
                  { label: 'Backend label', value: client.backendLabel, tone: 'good' },
                  { label: 'Stores', value: overview.stores.length },
                  { label: 'Resources', value: resourceCount },
                  { label: 'Recent audit events', value: recentAuditEvents },
                ]}
              />
            </Card>

            <Card title="Stores" eyebrow="Backend scope">
              <div className="grid gap-4 lg:grid-cols-2">
                {overview.stores.map((store) => (
                  <div key={store.store} className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="flex items-center justify-between gap-4">
                      <div>
                        <div className="text-sm text-[var(--muted)]">Store</div>
                        <div className="mt-1 text-xl font-semibold tracking-tight text-white">{store.store}</div>
                      </div>
                      <div className="text-emerald-300">reachable</div>
                    </div>
                    <div className="mt-4 grid grid-cols-2 gap-3 text-sm text-[var(--muted)]">
                      <div>
                        <div>Resources</div>
                        <div className="mt-1 text-lg font-semibold text-white">{store.resource_count}</div>
                      </div>
                      <div>
                        <div>Mutations</div>
                        <div className="mt-1 text-lg font-semibold text-white">{store.active_mutation_windows}</div>
                      </div>
                    </div>
                    <div className="mt-5 flex flex-wrap gap-3">
                      <Link href={`/s/${store.store}/resources`} className="rounded-xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-2 text-sm font-medium text-cyan-100">
                        Open resources
                      </Link>
                      <Link href={`/s/${store.store}/wrappers`} className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm text-white/90">
                        Open wrappers
                      </Link>
                      <Link href={`/s/${store.store}/audit`} className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm text-white/90">
                        Open audit
                      </Link>
                    </div>
                  </div>
                ))}
              </div>
            </Card>
          </div>

          <Card title="Explorer focus" eyebrow="Resource UX">
            <MetricGrid
              metrics={[
                { label: 'Backend scope', value: 'Single', tone: 'good' },
                { label: 'Store summaries', value: 'Live', tone: 'good' },
                { label: 'Resource index', value: 'Live', tone: 'good' },
                { label: 'Reveal + writes', value: 'Live', tone: 'good' },
              ]}
            />
          </Card>
        </div>
      </Shell>
    );
  } catch {
    return (
      <Shell
        title="Operator dashboard"
        description="The configured backend is present, but its overview endpoint is not currently reachable."
        section="overview"
      >
        <Card title="Backend unavailable" eyebrow="Live overview">
          <p className="text-sm text-[var(--muted)]">
            Check backend reachability, bearer token wiring, and store configuration, then refresh once the admin API is reachable again.
          </p>
        </Card>
      </Shell>
    );
  }
}
