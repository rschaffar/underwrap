import { Card } from '../../../../../../../components/card';
import { Shell } from '../../../../../../../components/shell';
import { buildResourcePath } from '../../../../../../../lib/mock-data';

function SlotCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
      <div className="text-sm font-medium text-white">{title}</div>
      <div className="mt-3 text-sm text-[var(--muted)]">{children}</div>
    </div>
  );
}

export default async function ResourceDetailPage({
  params,
}: {
  params: Promise<{ target: string; store: string; resource: string[] }>;
}) {
  const { target, store, resource } = await params;
  const resourcePath = buildResourcePath(resource);

  return (
    <Shell
      title={`Resource ${resourcePath}`}
      description="Detail scaffold for env, files, and libsecret entries, with room for mutation state, access matrix context, and recent audit activity."
      target={target}
      store={store}
      section="resources"
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.5fr)_minmax(320px,0.9fr)]">
        <Card title="Resource entries" eyebrow="Slot views">
          <div className="grid gap-4 lg:grid-cols-3">
            <SlotCard title="env">
              GH_TOKEN — masked by default, reveal/copy and write flow land in ticket 100.
            </SlotCard>
            <SlotCard title="files">
              hosts.json — preview/download/upload slot prepared for real binary-safe file workflows.
            </SlotCard>
            <SlotCard title="libsecret">
              default/item_1 — structured item view and reveal actions land in later tickets.
            </SlotCard>
          </div>
        </Card>

        <div className="grid gap-6">
          <Card title="Mutation window" eyebrow="Sidebar scaffold">
            <p className="text-sm text-[var(--muted)]">
              Show wrapper-scoped mutation state, expiry, and enable/disable actions here.
            </p>
          </Card>
          <Card title="Recent audit" eyebrow="Sidebar scaffold">
            <p className="text-sm text-[var(--muted)]">
              Read-only reveal/download/write history for this resource lands in ticket 116.
            </p>
          </Card>
        </div>
      </div>
    </Shell>
  );
}
