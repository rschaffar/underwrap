import Link from 'next/link';

import { Card } from '../components/card';
import { MetricGrid } from '../components/metric-grid';
import { Shell } from '../components/shell';
import { getConfiguredBackendClients } from '../lib/server/backend-client.ts';

export default async function HomePage() {
  const clients = await getConfiguredBackendClients();
  const targets = await Promise.all(
    clients.map(async (client) => {
      try {
        const overview = await client.getOverview();
        return {
          slug: client.target,
          status: 'reachable' as const,
          stores: overview.stores,
        };
      } catch {
        return {
          slug: client.target,
          status: 'degraded' as const,
          stores: [],
        };
      }
    }),
  );

  return (
    <Shell
      title="Operator dashboard"
      description="Configured backend targets, live store summaries, and the entry point into the new resource explorer."
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.8fr)]">
        <Card title="Backend targets" eyebrow="Live overview">
          <div className="grid gap-4 lg:grid-cols-2">
            {targets.map((target) => {
              const resourceCount = target.stores.reduce((sum, store) => sum + store.resource_count, 0);
              return (
                <Link
                  key={target.slug}
                  href={`/t/${target.slug}`}
                  className="rounded-2xl border border-white/10 bg-black/20 p-4 transition hover:border-cyan-400/30 hover:bg-cyan-400/8"
                >
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <div className="text-sm text-[var(--muted)]">Target</div>
                      <div className="mt-1 text-xl font-semibold tracking-tight text-white">{target.slug}</div>
                    </div>
                    <div className={target.status === 'reachable' ? 'text-emerald-300' : 'text-amber-300'}>
                      {target.status}
                    </div>
                  </div>
                  <div className="mt-4 grid grid-cols-2 gap-3 text-sm text-[var(--muted)]">
                    <div>
                      <div>Stores</div>
                      <div className="mt-1 text-lg font-semibold text-white">{target.stores.length}</div>
                    </div>
                    <div>
                      <div>Resources</div>
                      <div className="mt-1 text-lg font-semibold text-white">{resourceCount}</div>
                    </div>
                  </div>
                </Link>
              );
            })}
          </div>
        </Card>

        <Card title="Explorer focus" eyebrow="Resource UX">
          <MetricGrid
            metrics={[
              { label: 'Target switching', value: 'Live', tone: 'good' },
              { label: 'Store summaries', value: 'Live', tone: 'good' },
              { label: 'Resource index', value: 'Live', tone: 'good' },
              { label: 'Reveal + writes', value: 'Next ticket', tone: 'warn' },
            ]}
          />
        </Card>
      </div>
    </Shell>
  );
}
