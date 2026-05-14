import { Card } from '../../../../../../../components/card';
import { ResourceEntryTable } from '../../../../../../../components/resource-entry-table';
import { Shell } from '../../../../../../../components/shell';
import { buildResourcePath } from '../../../../../../../lib/mock-data';
import { requireBackendClient } from '../../../../../../../lib/server/navigation.ts';

function SidebarCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card title={title} eyebrow="Next step">
      <div className="text-sm text-[var(--muted)]">{children}</div>
    </Card>
  );
}

export default async function ResourceDetailPage({
  params,
}: {
  params: Promise<{ target: string; store: string; resource: string[] }>;
}) {
  const { target, store, resource } = await params;
  const resourcePath = buildResourcePath(resource);
  const client = await requireBackendClient(target);

  if (!client) {
    return (
      <Shell
        title={`Resource ${resourcePath}`}
        description="The selected target is not configured in the runtime backend list."
        target={target}
        store={store}
        section="resources"
      >
        <Card title="Unknown target" eyebrow="Resource detail">
          <p className="text-sm text-[var(--muted)]">Select a configured target to inspect resource details.</p>
        </Card>
      </Shell>
    );
  }

  let resourceDetail;
  try {
    resourceDetail = await client.getResourceDetail(store, resourcePath);
  } catch {
    return (
      <Shell
        title={`Resource ${resourcePath}`}
        description="The backend resource detail endpoint could not load this resource right now."
        target={target}
        store={store}
        section="resources"
      >
        <Card title="Resource detail unavailable" eyebrow="Resource detail">
          <p className="text-sm text-[var(--muted)]">
            Check that the store and resource exist, then try again once the backend is reachable.
          </p>
        </Card>
      </Shell>
    );
  }

  return (
    <Shell
      title={`Resource ${resourceDetail.resource}`}
      description="Live slot metadata for env, files, and libsecret entries, with file entries presented as real file resources rather than raw textareas."
      target={target}
      store={store}
      section="resources"
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.6fr)_minmax(320px,0.9fr)]">
        <div className="grid gap-6">
          <ResourceEntryTable
            title="env"
            entries={resourceDetail.slots.env}
            emptyLabel="No env entries stored for this resource."
            actions={() => <span className="text-[var(--muted)]">Reveal in next ticket</span>}
          />
          <ResourceEntryTable
            title="files"
            entries={resourceDetail.slots.files}
            emptyLabel="No file entries stored for this resource."
            actions={(entry) => (
              <div className="flex flex-wrap gap-3 text-sm">
                <span className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-[var(--muted)]">{entry.is_utf8 ? 'preview later' : 'binary file'}</span>
                <span className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-[var(--muted)]">download later</span>
              </div>
            )}
          />
          <ResourceEntryTable
            title="libsecret"
            entries={resourceDetail.slots.libsecret}
            emptyLabel="No libsecret items stored for this resource."
            actions={() => <span className="text-[var(--muted)]">Reveal in next ticket</span>}
          />
        </div>

        <div className="grid gap-6">
          <SidebarCard title="Mutation window">
            Mutation status and write controls land in ticket 100; this page now provides the live slot inventory those actions will work against.
          </SidebarCard>
          <SidebarCard title="Files as files">
            File entries already surface as file metadata with size and text/binary classification so the next ticket can add preview, upload, and download cleanly.
          </SidebarCard>
        </div>
      </div>
    </Shell>
  );
}
