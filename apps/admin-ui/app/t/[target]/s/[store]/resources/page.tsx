import Link from 'next/link';

import { Card } from '../../../../../../components/card';
import { Shell } from '../../../../../../components/shell';
import { getStore } from '../../../../../../lib/mock-data';

const resources = [
  { name: 'gh/main', slots: ['env', 'libsecret'], mutation: 'active' },
  { name: 'aws/prod', slots: ['env', 'files'], mutation: 'locked' },
  { name: 'gws/castlabs', slots: ['files'], mutation: 'locked' },
];

export default async function ResourcesPage({
  params,
}: {
  params: Promise<{ target: string; store: string }>;
}) {
  const { target, store } = await params;
  const selectedStore = getStore(target, store);

  return (
    <Shell
      title={`Resources / ${store}`}
      description="Searchable resource inventory scaffold with room for slot presence, mutation state, and recent audit context."
      target={target}
      store={store}
      section="resources"
    >
      <Card title="Resource inventory" eyebrow="Placeholder data">
        <div className="mb-4 flex flex-col gap-3 sm:flex-row">
          <input
            type="search"
            placeholder="Search resources, wrappers, slots…"
            className="min-w-0 flex-1 rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
          />
          <button className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm text-white/90">Filter</button>
        </div>

        <div className="overflow-hidden rounded-2xl border border-white/10">
          <table className="min-w-full divide-y divide-white/10 text-left text-sm">
            <thead className="bg-black/30 text-[var(--muted)]">
              <tr>
                <th className="px-4 py-3 font-medium">Resource</th>
                <th className="px-4 py-3 font-medium">Slots</th>
                <th className="px-4 py-3 font-medium">Mutation</th>
                <th className="px-4 py-3 font-medium">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10 bg-black/10">
              {resources.map((resource) => (
                <tr key={resource.name}>
                  <td className="px-4 py-4 text-white">{resource.name}</td>
                  <td className="px-4 py-4 text-[var(--muted)]">{resource.slots.join(', ')}</td>
                  <td className="px-4 py-4">
                    <span className={resource.mutation === 'active' ? 'rounded-full border border-amber-400/30 bg-amber-400/10 px-2.5 py-1 text-xs font-medium text-amber-200' : 'rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-xs font-medium text-[var(--muted)]'}>
                      {resource.mutation}
                    </span>
                  </td>
                  <td className="px-4 py-4">
                    <Link href={`/t/${target}/s/${store}/resources/${resource.name}`} className="text-cyan-200 hover:text-cyan-100">
                      Open detail
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {selectedStore ? (
          <p className="mt-4 text-sm text-[var(--muted)]">
            Store scaffold based on <span className="text-white">{selectedStore.store}</span>; backend-driven listing lands in the UI endpoint tickets.
          </p>
        ) : null}
      </Card>
    </Shell>
  );
}
