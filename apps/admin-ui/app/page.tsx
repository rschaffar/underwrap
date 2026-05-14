import Link from 'next/link';

import { Card } from '@/components/card';
import { MetricGrid } from '@/components/metric-grid';
import { Shell } from '@/components/shell';
import { getTargets } from '@/lib/mock-data';

export default function HomePage() {
  const targets = getTargets();

  return (
    <Shell
      title="Operator dashboard"
      description="This scaffold establishes the multi-target shell and dark ops-console design direction for the Next.js rewrite."
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.8fr)]">
        <Card title="Backend targets" eyebrow="Overview">
          <div className="grid gap-4 lg:grid-cols-2">
            {targets.map((target) => (
              <Link
                key={target.slug}
                href={`/t/${target.slug}`}
                className="rounded-2xl border border-white/10 bg-black/20 p-4 transition hover:border-cyan-400/30 hover:bg-cyan-400/8"
              >
                <div className="flex items-center justify-between gap-4">
                  <div>
                    <div className="text-sm text-[var(--muted)]">Target</div>
                    <div className="mt-1 text-xl font-semibold tracking-tight text-white">{target.label}</div>
                  </div>
                  <div className={target.status === 'reachable' ? 'text-emerald-300' : 'text-amber-300'}>
                    {target.status === 'reachable' ? 'reachable' : 'degraded'}
                  </div>
                </div>
                <div className="mt-4 grid grid-cols-2 gap-3 text-sm text-[var(--muted)]">
                  <div>
                    <div>Stores</div>
                    <div className="mt-1 text-lg font-semibold text-white">{target.stores.length}</div>
                  </div>
                  <div>
                    <div>Resources</div>
                    <div className="mt-1 text-lg font-semibold text-white">
                      {target.stores.reduce((sum, store) => sum + store.resourceCount, 0)}
                    </div>
                  </div>
                </div>
              </Link>
            ))}
          </div>
        </Card>

        <Card title="Scaffold objectives" eyebrow="Ticket 010">
          <MetricGrid
            metrics={[
              { label: 'App Router', value: 'Ready', tone: 'good' },
              { label: 'Multi-target shell', value: 'Ready', tone: 'good' },
              { label: 'Design direction', value: 'Dark ops', tone: 'good' },
              { label: 'Backend wiring', value: 'Next ticket', tone: 'warn' },
            ]}
          />
        </Card>
      </div>
    </Shell>
  );
}
