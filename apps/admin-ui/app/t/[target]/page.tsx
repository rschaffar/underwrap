import Link from 'next/link';
import { notFound } from 'next/navigation';

import { Card } from '../../../components/card';
import { MetricGrid } from '../../../components/metric-grid';
import { Shell } from '../../../components/shell';
import { getTarget } from '../../../lib/mock-data';

export default async function TargetPage({ params }: { params: Promise<{ target: string }> }) {
  const { target } = await params;
  const selectedTarget = getTarget(target);

  if (!selectedTarget || selectedTarget.slug !== target) {
    notFound();
  }

  return (
    <Shell
      title={`Target ${selectedTarget.label}`}
      description="Store-level overview page scaffold with the structure needed for backend summaries, mutation counts, and recent audit activity."
      target={target}
      section="overview"
    >
      <div className="grid gap-6">
        {selectedTarget.stores.map((store) => (
          <Card key={store.store} title={store.store} eyebrow="Store summary">
            <MetricGrid
              metrics={[
                { label: 'Resources', value: store.resourceCount },
                { label: 'Unlocked wrappers', value: store.wrapperCounts.unlocked, tone: 'good' },
                { label: 'Per-call approval', value: store.wrapperCounts.perCallApproval, tone: 'warn' },
                { label: 'Active mutation windows', value: store.activeMutationWindows, tone: store.activeMutationWindows > 0 ? 'warn' : 'default' },
              ]}
            />
            <div className="mt-5 flex flex-wrap gap-3">
              <Link href={`/t/${target}/s/${store.store}/resources`} className="rounded-xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-2 text-sm font-medium text-cyan-100">
                Open resources
              </Link>
              <Link href={`/t/${target}/s/${store.store}/wrappers`} className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm text-white/90">
                Open wrappers
              </Link>
              <Link href={`/t/${target}/s/${store.store}/audit`} className="rounded-xl border border-white/10 bg-white/5 px-4 py-2 text-sm text-white/90">
                Open audit
              </Link>
            </div>
          </Card>
        ))}
      </div>
    </Shell>
  );
}
