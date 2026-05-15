import { Card } from '../../../../../components/card';
import { ResourceEntryTable } from '../../../../../components/resource-entry-table';
import { RevealControl } from '../../../../../components/reveal-control';
import { Shell } from '../../../../../components/shell';
import { requireSession } from '../../../../../lib/auth-session.ts';
import { buildResourcePath } from '../../../../../lib/mock-data';
import { requireBackendClient } from '../../../../../lib/server/navigation.ts';
import type { UiMutationWrapperState, UiResourceEntrySummary } from '../../../../../lib/server/ui-types.ts';

function SidebarCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card title={title} eyebrow="Resource operations">
      <div className="text-sm text-[var(--muted)]">{children}</div>
    </Card>
  );
}

function flashInfo(flash: string | undefined) {
  switch (flash) {
    case 'mutation-enabled':
      return { tone: 'warn', text: 'Mutation window enabled for the selected wrapper and entry.' };
    case 'mutation-disabled':
      return { tone: 'default', text: 'Mutation window revoked for the selected wrapper and entry.' };
    case 'secret-saved':
      return { tone: 'good', text: 'Secret material stored successfully.' };
    case 'secret-deleted':
      return { tone: 'good', text: 'Stored entry deleted successfully.' };
    case 'mutation-denied':
      return { tone: 'danger', text: 'Write/delete denied. Enable a mutation window for the chosen wrapper and entry first.' };
    case 'payload-too-large':
      return { tone: 'danger', text: 'The payload exceeds the 1 MiB v1 limit.' };
    case 'upload-failed':
    case 'write-failed':
    case 'delete-failed':
    case 'mutation-failed':
      return { tone: 'danger', text: 'The requested mutation action failed.' };
    default:
      return null;
  }
}

function formatEpoch(epoch?: number | null) {
  if (!epoch) {
    return '—';
  }
  return new Date(epoch * 1000).toISOString().replace('T', ' ').replace('.000Z', 'Z');
}

function WrapperSelect({ wrappers, name = 'wrapper' }: { wrappers: UiMutationWrapperState[]; name?: string }) {
  return (
    <select
      name={name}
      className="rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-white focus:border-cyan-400/40 focus:outline-none"
      defaultValue={wrappers[0]?.wrapper ?? ''}
    >
      {wrappers.map((wrapper) => (
        <option key={wrapper.wrapper} value={wrapper.wrapper}>
          {wrapper.wrapper}
        </option>
      ))}
    </select>
  );
}

function EntryMutationForm({
  store,
  resource,
  csrfToken,
  wrappers,
  entry,
  allowDownload = false,
}: {
  store: string;
  resource: string;
  csrfToken: string;
  wrappers: UiMutationWrapperState[];
  entry: UiResourceEntrySummary;
  allowDownload?: boolean;
}) {
  if (wrappers.length === 0) {
    return allowDownload ? (
      <form className="grid gap-2" method="post">
        <input type="hidden" name="store" value={store} />
        <input type="hidden" name="resource" value={resource} />
        <input type="hidden" name="secret_id" value={entry.secret_id} />
        <input type="hidden" name="csrf_token" value={csrfToken} />
        <button
          type="submit"
          formAction="/actions/resource/download"
          className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-xs text-white/90"
        >
          Download
        </button>
      </form>
    ) : (
      <div className="text-xs text-[var(--muted)]">No wrapper context is available for mutation or delete actions.</div>
    );
  }

  return (
    <form className="grid gap-2" method="post">
      <input type="hidden" name="store" value={store} />
      <input type="hidden" name="resource" value={resource} />
      <input type="hidden" name="secret_id" value={entry.secret_id} />
      <input type="hidden" name="csrf_token" value={csrfToken} />
      {wrappers.length > 0 ? <WrapperSelect wrappers={wrappers} /> : null}
      <div className="flex flex-wrap gap-2 text-xs">
        <button
          type="submit"
          formAction="/actions/resource/mutation"
          name="mode"
          value="temporary"
          className="rounded-xl border border-amber-400/30 bg-amber-400/10 px-3 py-1.5 text-amber-200"
        >
          Enable 30m
        </button>
        <button
          type="submit"
          formAction="/actions/resource/mutation"
          name="mode"
          value="disabled"
          className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-white/90"
        >
          Revoke
        </button>
        <button
          type="submit"
          formAction="/actions/resource/delete"
          className="rounded-xl border border-rose-400/30 bg-rose-400/10 px-3 py-1.5 text-rose-200"
        >
          Delete
        </button>
        {allowDownload ? (
          <button
            type="submit"
            formAction="/actions/resource/download"
            className="rounded-xl border border-white/10 bg-white/5 px-3 py-1.5 text-white/90"
          >
            Download
          </button>
        ) : null}
      </div>
    </form>
  );
}

function TextWriteForm({
  title,
  store,
  resource,
  slot,
  wrappers,
  csrfToken,
  entryLabel,
  placeholder,
}: {
  title: string;
  store: string;
  resource: string;
  slot: 'env' | 'libsecret';
  wrappers: UiMutationWrapperState[];
  csrfToken: string;
  entryLabel: string;
  placeholder: string;
}) {
  return (
    <Card title={title} eyebrow="Write flow">
      {wrappers.length === 0 ? (
        <p className="text-sm text-[var(--muted)]">No catalog wrappers are associated with this resource yet.</p>
      ) : (
        <form className="grid gap-3" method="post">
          <input type="hidden" name="store" value={store} />
          <input type="hidden" name="resource" value={resource} />
          <input type="hidden" name="slot" value={slot} />
          <input type="hidden" name="csrf_token" value={csrfToken} />
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            Wrapper
            <WrapperSelect wrappers={wrappers} />
          </label>
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            {entryLabel}
            <input
              name="entry"
              placeholder={placeholder}
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
          </label>
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            Value
            <textarea
              name="value"
              rows={5}
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 font-mono text-sm text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
          </label>
          <div className="flex flex-wrap gap-3">
            <button
              type="submit"
              formAction="/actions/resource/mutation"
              name="mode"
              value="temporary"
              className="rounded-2xl border border-amber-400/30 bg-amber-400/10 px-4 py-3 text-sm font-medium text-amber-100"
            >
              Enable 30m mutation
            </button>
            <button
              type="submit"
              formAction="/actions/resource/write-text"
              className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100"
            >
              Save text entry
            </button>
          </div>
        </form>
      )}
    </Card>
  );
}

function FileWriteForm({
  store,
  resource,
  wrappers,
  csrfToken,
}: {
  store: string;
  resource: string;
  wrappers: UiMutationWrapperState[];
  csrfToken: string;
}) {
  return (
    <Card title="files upload / replace" eyebrow="Write flow">
      {wrappers.length === 0 ? (
        <p className="text-sm text-[var(--muted)]">No catalog wrappers are associated with this resource yet.</p>
      ) : (
        <form className="grid gap-3" method="post" encType="multipart/form-data">
          <input type="hidden" name="store" value={store} />
          <input type="hidden" name="resource" value={resource} />
          <input type="hidden" name="slot" value="files" />
          <input type="hidden" name="csrf_token" value={csrfToken} />
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            Wrapper
            <WrapperSelect wrappers={wrappers} />
          </label>
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            Relative path
            <input
              name="entry"
              placeholder="config.json"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
          </label>
          <label className="grid gap-2 text-sm text-[var(--muted)]">
            File
            <input
              type="file"
              name="file"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-sm text-white file:mr-3 file:rounded-xl file:border-0 file:bg-cyan-400/15 file:px-3 file:py-2 file:text-cyan-100"
            />
          </label>
          <div className="flex flex-wrap gap-3">
            <button
              type="submit"
              formAction="/actions/resource/mutation"
              name="mode"
              value="temporary"
              className="rounded-2xl border border-amber-400/30 bg-amber-400/10 px-4 py-3 text-sm font-medium text-amber-100"
            >
              Enable 30m mutation
            </button>
            <button
              type="submit"
              formAction="/actions/resource/upload-file"
              className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100"
            >
              Upload / replace file
            </button>
          </div>
        </form>
      )}
    </Card>
  );
}

export default async function ResourceDetailPage({
  params,
  searchParams,
}: {
  params: Promise<{ store: string; resource: string[] }>;
  searchParams: Promise<{ flash?: string }>;
}) {
  const [{ store, resource }, query, session] = await Promise.all([params, searchParams, requireSession()]);
  const resourcePath = buildResourcePath(resource);
  const client = await requireBackendClient();

  let resourceDetail;
  let mutationState;
  try {
    [resourceDetail, mutationState] = await Promise.all([
      client.getResourceDetail(store, resourcePath),
      client.getMutationState(store, resourcePath),
    ]);
  } catch {
    return (
      <Shell
        title={`Resource ${resourcePath}`}
        description="The backend resource detail endpoint could not load this resource right now."
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

  const flash = flashInfo(query.flash);

  return (
    <Shell
      title={`Resource ${resourceDetail.resource}`}
      description="Explicit reveal, file download/upload, and mutation-gated write/delete flows for env, files, and libsecret entries."
      store={store}
      section="resources"
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.6fr)_minmax(320px,0.9fr)]">
        <div className="grid gap-6">
          {flash ? (
            <Card title="Operation status" eyebrow="Resource action">
              <div className={flash.tone === 'good'
                ? 'text-sm text-emerald-200'
                : flash.tone === 'warn'
                  ? 'text-sm text-amber-200'
                  : flash.tone === 'danger'
                    ? 'text-sm text-rose-200'
                    : 'text-sm text-[var(--muted)]'}>
                {flash.text}
              </div>
            </Card>
          ) : null}

          <ResourceEntryTable
            title="env"
            entries={resourceDetail.slots.env}
            emptyLabel="No env entries stored for this resource."
            actions={(entry) => (
              <div className="grid gap-3">
                <RevealControl store={store} secretId={entry.secret_id} csrfToken={session.csrfToken} />
                <EntryMutationForm
                  store={store}
                  resource={resourceDetail.resource}
                  csrfToken={session.csrfToken}
                  wrappers={mutationState.wrappers}
                  entry={entry}
                />
              </div>
            )}
          />

          <TextWriteForm
            title="env write / replace"
            store={store}
            resource={resourceDetail.resource}
            slot="env"
            wrappers={mutationState.wrappers}
            csrfToken={session.csrfToken}
            entryLabel="Environment variable"
            placeholder="GH_TOKEN"
          />

          <ResourceEntryTable
            title="files"
            entries={resourceDetail.slots.files}
            emptyLabel="No file entries stored for this resource."
            actions={(entry) => (
              <div className="grid gap-3">
                <RevealControl store={store} secretId={entry.secret_id} csrfToken={session.csrfToken} allowDownload />
                <EntryMutationForm
                  store={store}
                  resource={resourceDetail.resource}
                  csrfToken={session.csrfToken}
                  wrappers={mutationState.wrappers}
                  entry={entry}
                  allowDownload
                />
              </div>
            )}
          />

          <FileWriteForm
            store={store}
            resource={resourceDetail.resource}
            wrappers={mutationState.wrappers}
            csrfToken={session.csrfToken}
          />

          <ResourceEntryTable
            title="libsecret"
            entries={resourceDetail.slots.libsecret}
            emptyLabel="No libsecret items stored for this resource."
            actions={(entry) => (
              <div className="grid gap-3">
                <RevealControl store={store} secretId={entry.secret_id} csrfToken={session.csrfToken} />
                <EntryMutationForm
                  store={store}
                  resource={resourceDetail.resource}
                  csrfToken={session.csrfToken}
                  wrappers={mutationState.wrappers}
                  entry={entry}
                />
              </div>
            )}
          />

          <TextWriteForm
            title="libsecret write / replace"
            store={store}
            resource={resourceDetail.resource}
            slot="libsecret"
            wrappers={mutationState.wrappers}
            csrfToken={session.csrfToken}
            entryLabel="Relative item id"
            placeholder="default/item_1"
          />
        </div>

        <div className="grid gap-6">
          <SidebarCard title="Mutation state by wrapper">
            <div className="grid gap-3">
              {mutationState.wrappers.length > 0 ? (
                mutationState.wrappers.map((wrapper) => (
                  <div key={wrapper.wrapper} className="rounded-2xl border border-white/10 bg-black/20 p-3">
                    <div className="flex items-center justify-between gap-3 text-sm">
                      <div className="font-medium text-white">{wrapper.wrapper}</div>
                      <span className={wrapper.active ? 'rounded-full border border-amber-400/30 bg-amber-400/10 px-2.5 py-1 text-xs text-amber-200' : 'rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-xs text-[var(--muted)]'}>
                        {wrapper.mode}
                      </span>
                    </div>
                    <div className="mt-2 text-xs text-[var(--muted)]">Active until: {formatEpoch(wrapper.expires_at_epoch)}</div>
                  </div>
                ))
              ) : (
                <div>No catalog wrappers are currently attached to this resource.</div>
              )}
            </div>
          </SidebarCard>
          <SidebarCard title="Flow guide">
            Enable a short-lived mutation window for the specific wrapper + entry, write or upload the new value, verify via reveal/download, then revoke the mutation window.
          </SidebarCard>
          <SidebarCard title="Secret handling">
            Reveals are explicit, masked by default, never placed in the URL, and downloads go through a no-store POST route. File uploads are capped at 1 MiB in v1.
          </SidebarCard>
        </div>
      </div>
    </Shell>
  );
}
