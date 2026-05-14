import Link from 'next/link';

import { Card } from '../../../../../../components/card';
import { Shell } from '../../../../../../components/shell';
import { requireBackendClient } from '../../../../../../lib/server/navigation.ts';

export default async function ResourcesPage({
  params,
  searchParams,
}: {
  params: Promise<{ target: string; store: string }>;
  searchParams: Promise<{ q?: string; slot?: string }>;
}) {
  const [{ target, store }, filters] = await Promise.all([params, searchParams]);
  const client = await requireBackendClient(target);

  if (!client) {
    return (
      <Shell
        title={`Resources / ${store}`}
        description="The selected target is not configured in the runtime backend list."
        target={target}
        store={store}
        section="resources"
      >
        <Card title="Unknown target" eyebrow="Resource explorer">
          <p className="text-sm text-[var(--muted)]">Select a configured target to browse resources.</p>
        </Card>
      </Shell>
    );
  }

  let response;
  try {
    response = await client.queryResources(store, filters);
  } catch {
    return (
      <Shell
        title={`Resources / ${store}`}
        description="The resource index endpoint could not be loaded for this store."
        target={target}
        store={store}
        section="resources"
      >
        <Card title="Resource index unavailable" eyebrow="Resource explorer">
          <p className="text-sm text-[var(--muted)]">
            Check that the store exists for this backend target and that the UI read-model endpoints are reachable.
          </p>
        </Card>
      </Shell>
    );
  }

  return (
    <Shell
      title={`Resources / ${store}`}
      description="Searchable live resource inventory with slot presence and mutation-state cues from the backend UI read model."
      target={target}
      store={store}
      section="resources"
    >
      <Card title="Resource inventory" eyebrow="Live data">
        <form className="mb-4 flex flex-col gap-3 sm:flex-row" method="get">
          <input type="hidden" name="slot" value={filters.slot ?? ''} />
          <input
            type="search"
            name="q"
            defaultValue={filters.q ?? ''}
            placeholder="Search resources or wrappers…"
            className="min-w-0 flex-1 rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
          />
          <select
            name="slot"
            defaultValue={filters.slot ?? 'any'}
            className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm text-white focus:border-cyan-400/40 focus:outline-none"
          >
            <option value="any">All slots</option>
            <option value="env">env</option>
            <option value="files">files</option>
            <option value="libsecret">libsecret</option>
          </select>
          <button className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100">
            Filter
          </button>
        </form>

        <div className="overflow-hidden rounded-2xl border border-white/10">
          <table className="min-w-full divide-y divide-white/10 text-left text-sm">
            <thead className="bg-black/30 text-[var(--muted)]">
              <tr>
                <th className="px-4 py-3 font-medium">Resource</th>
                <th className="px-4 py-3 font-medium">Slots</th>
                <th className="px-4 py-3 font-medium">Wrappers</th>
                <th className="px-4 py-3 font-medium">Mutation</th>
                <th className="px-4 py-3 font-medium">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10 bg-black/10">
              {response.items.map((resource) => (
                <tr key={resource.resource}>
                  <td className="px-4 py-4 text-white">{resource.resource}</td>
                  <td className="px-4 py-4 text-[var(--muted)]">
                    {[
                      resource.slots.env && 'env',
                      resource.slots.files && 'files',
                      resource.slots.libsecret && 'libsecret',
                    ]
                      .filter(Boolean)
                      .join(', ')}
                  </td>
                  <td className="px-4 py-4 text-[var(--muted)]">{resource.catalog_wrappers.join(', ') || '—'}</td>
                  <td className="px-4 py-4">
                    <span className={resource.mutation.active ? 'rounded-full border border-amber-400/30 bg-amber-400/10 px-2.5 py-1 text-xs font-medium text-amber-200' : 'rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-xs font-medium text-[var(--muted)]'}>
                      {resource.mutation.active ? 'active' : 'inactive'}
                    </span>
                  </td>
                  <td className="px-4 py-4">
                    <Link href={`/t/${target}/s/${store}/resources/${resource.resource}`} className="text-cyan-200 hover:text-cyan-100">
                      Open detail
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <p className="mt-4 text-sm text-[var(--muted)]">Showing {response.items.length} of {response.total} matching resources.</p>
      </Card>
    </Shell>
  );
}
