import Link from 'next/link';
import { notFound } from 'next/navigation';

import { Card } from '../../../components/card';
import { MetricGrid } from '../../../components/metric-grid';
import { Shell } from '../../../components/shell';
import { requireBackendClient } from '../../../lib/server/navigation.ts';

export default async function TargetPage({ params }: { params: Promise<{ target: string }> }) {
  const { target } = await params;
  const client = await requireBackendClient(target);
  if (!client) {
    notFound();
  }

  let overview;
  try {
    overview = await client.getOverview();
  } catch {
    return (
      <Shell
        title={`Target ${target}`}
        description="The backend target is configured, but its overview endpoint is not currently reachable."
        target={target}
        section="overview"
      >
        <Card title="Backend unavailable" eyebrow="Target overview">
          <p className="text-sm text-[var(--muted)]">
            The target could not be queried right now. Check backend reachability, bearer token wiring, and store configuration.
          </p>
        </Card>
      </Shell>
    );
  }

  return (
    <Shell
      title={`Target ${target}`}
      description="Live store summaries from the backend overview endpoint, with direct entry points into resource, wrapper, and audit workflows."
      target={target}
      section="overview"
    >
      <div className="grid gap-6">
        {overview.stores.map((store) => (
          <Card key={store.store} title={store.store} eyebrow="Store summary">
            <MetricGrid
              metrics={[
                { label: 'Resources', value: store.resource_count },
                { label: 'Unlocked wrappers', value: store.wrapper_counts.unlocked, tone: 'good' },
                { label: 'Per-call approval', value: store.wrapper_counts.per_call_approval, tone: 'warn' },
                {
                  label: 'Active mutation windows',
                  value: store.active_mutation_windows,
                  tone: store.active_mutation_windows > 0 ? 'warn' : 'default',
                },
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
