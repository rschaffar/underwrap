import { Card } from '../../../../components/card';
import { Shell } from '../../../../components/shell';
import { WrapperModeForm } from '../../../../components/wrapper-mode-form';
import { requireSession } from '../../../../lib/auth-session.ts';
import { requireBackendClient } from '../../../../lib/server/navigation.ts';
import type { UiMatrixRow, UiWrapperApprovalMode, UiWrapperListItem } from '../../../../lib/server/ui-types.ts';

function modeLabel(mode: UiWrapperApprovalMode) {
  switch (mode.kind) {
    case 'locked':
      return 'locked';
    case 'per-call-approval':
      return 'per-call approval';
    case 'unlocked':
      return mode.until_epoch ? `unlocked until ${new Date(mode.until_epoch * 1000).toISOString().replace('T', ' ').replace('.000Z', 'Z')}` : 'unlocked';
  }
}

function modeTone(mode: UiWrapperApprovalMode) {
  switch (mode.kind) {
    case 'locked':
      return 'rounded-full border border-rose-400/30 bg-rose-400/10 px-2.5 py-1 text-xs font-medium text-rose-200';
    case 'per-call-approval':
      return 'rounded-full border border-amber-400/30 bg-amber-400/10 px-2.5 py-1 text-xs font-medium text-amber-200';
    case 'unlocked':
      return mode.until_epoch
        ? 'rounded-full border border-fuchsia-400/30 bg-fuchsia-400/10 px-2.5 py-1 text-xs font-medium text-fuchsia-200'
        : 'rounded-full border border-emerald-400/30 bg-emerald-400/10 px-2.5 py-1 text-xs font-medium text-emerald-200';
  }
}

function flashInfo(flash: string | undefined) {
  switch (flash) {
    case 'wrapper-mode-saved':
      return { tone: 'good', text: 'Wrapper mode updated successfully.' };
    case 'wrapper-mode-invalid-until':
      return { tone: 'danger', text: 'Choose a valid future date/time for unlock-until.' };
    case 'wrapper-mode-missing-key':
      return { tone: 'danger', text: 'Per-call approval mode requires an OpenSSH public key.' };
    case 'wrapper-mode-invalid':
    case 'wrapper-mode-failed':
      return { tone: 'danger', text: 'The wrapper mode update failed.' };
    case 'matrix-cell-enabled':
      return { tone: 'good', text: 'Matrix allow cell enabled.' };
    case 'matrix-cell-disabled':
      return { tone: 'default', text: 'Matrix allow cell disabled.' };
    case 'matrix-toggle-failed':
      return { tone: 'danger', text: 'The matrix toggle failed.' };
    default:
      return null;
  }
}

function FlashCard({ flash }: { flash: ReturnType<typeof flashInfo> }) {
  if (!flash) {
    return null;
  }
  return (
    <Card title="Operation status" eyebrow="Wrapper console">
      <div className={flash.tone === 'good'
        ? 'text-sm text-emerald-200'
        : flash.tone === 'danger'
          ? 'text-sm text-rose-200'
          : 'text-sm text-[var(--muted)]'}>
        {flash.text}
      </div>
    </Card>
  );
}

function MatrixCellForm({
  store,
  resource,
  row,
  operation,
  enabled,
  csrfToken,
  query,
}: {
  store: string;
  resource: string;
  row: UiMatrixRow;
  operation: string;
  enabled: boolean;
  csrfToken: string;
  query: { q?: string; mode?: string; resource?: string; host?: string; user?: string; matrix_wrapper?: string };
}) {
  return (
    <form method="post">
      <input type="hidden" name="store" value={store} />
      <input type="hidden" name="resource" value={resource} />
      <input type="hidden" name="wrapper" value={row.wrapper} />
      <input type="hidden" name="operation" value={operation} />
      <input type="hidden" name="enabled" value={enabled ? 'false' : 'true'} />
      <input type="hidden" name="csrf_token" value={csrfToken} />
      <input type="hidden" name="q" value={query.q ?? ''} />
      <input type="hidden" name="mode" value={query.mode ?? ''} />
      <input type="hidden" name="host" value={query.host ?? ''} />
      <input type="hidden" name="user" value={query.user ?? ''} />
      <input type="hidden" name="matrix_wrapper" value={query.matrix_wrapper ?? ''} />
      <button
        type="submit"
        formAction="/actions/wrappers/matrix-set-cell"
        className={enabled
          ? 'min-w-16 rounded-xl border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-xs font-medium text-emerald-200'
          : 'min-w-16 rounded-xl border border-white/10 bg-white/5 px-2 py-1 text-xs font-medium text-[var(--muted)]'}
      >
        {enabled ? 'allow' : 'deny'}
      </button>
    </form>
  );
}

function WrapperRow({
  item,
  store,
  csrfToken,
  query,
}: {
  item: UiWrapperListItem;
  store: string;
  csrfToken: string;
  query: { q?: string; mode?: string; resource?: string; host?: string; user?: string; matrix_wrapper?: string };
}) {
  return (
    <div className="rounded-2xl border border-white/10 bg-black/10 p-4">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
        <div>
          <div className="text-lg font-semibold text-white">{item.wrapper}</div>
          <div className="mt-2 flex flex-wrap items-center gap-3">
            <span className={modeTone(item.mode)}>{modeLabel(item.mode)}</span>
            <span className="text-sm text-[var(--muted)]">{item.resource_count} resources</span>
          </div>
          {item.reason ? <div className="mt-2 text-sm text-[var(--muted)]">Reason: {item.reason}</div> : null}
        </div>
      </div>
      <div className="mt-4">
        <WrapperModeForm
          store={store}
          wrapper={item.wrapper}
          csrfToken={csrfToken}
          currentMode={item.mode}
          currentReason={item.reason}
          query={query}
        />
      </div>
    </div>
  );
}

export default async function WrappersPage({
  params,
  searchParams,
}: {
  params: Promise<{ store: string }>;
  searchParams: Promise<{
    q?: string;
    mode?: string;
    resource?: string;
    host?: string;
    user?: string;
    matrix_wrapper?: string;
    flash?: string;
  }>;
}) {
  const [{ store }, filters, session] = await Promise.all([params, searchParams, requireSession()]);
  const client = await requireBackendClient();

  let wrappersResponse;
  let resourcesResponse;
  try {
    [wrappersResponse, resourcesResponse] = await Promise.all([
      client.queryWrappers(store, { q: filters.q, mode: filters.mode }),
      client.queryResources(store, {}),
    ]);
  } catch {
    return (
      <Shell
        title={`Wrappers / ${store}`}
        description="The wrapper or matrix read models could not be loaded right now."
        store={store}
        section="wrappers"
      >
        <Card title="Wrapper console unavailable" eyebrow="Wrapper console">
          <p className="text-sm text-[var(--muted)]">Check backend reachability and store configuration, then try again.</p>
        </Card>
      </Shell>
    );
  }

  const selectedResource = filters.resource?.trim() || resourcesResponse.items[0]?.resource || '';
  let matrixResponse = null;
  if (selectedResource) {
    try {
      matrixResponse = await client.queryMatrix(store, {
        resource: selectedResource,
        host: filters.host,
        user: filters.user,
        wrapper: filters.matrix_wrapper,
      });
    } catch {
      matrixResponse = null;
    }
  }

  const flash = flashInfo(filters.flash);
  const query = {
    q: filters.q ?? '',
    mode: filters.mode ?? 'any',
    resource: selectedResource,
    host: filters.host ?? '',
    user: filters.user ?? '',
    matrix_wrapper: filters.matrix_wrapper ?? '',
  };

  return (
    <Shell
      title={`Wrappers / ${store}`}
      description="Live wrapper mode management plus a filterable access matrix for one selected resource at a time."
      store={store}
      section="wrappers"
    >
      <div className="grid gap-6">
        <FlashCard flash={flash} />

        <Card title="Wrapper modes" eyebrow="Live data">
          <form className="mb-5 grid gap-3 lg:grid-cols-[minmax(0,1fr)_220px_auto]" method="get">
            <input
              type="search"
              name="q"
              defaultValue={filters.q ?? ''}
              placeholder="Filter wrappers…"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
            <select
              name="mode"
              defaultValue={filters.mode ?? 'any'}
              className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm text-white focus:border-cyan-400/40 focus:outline-none"
            >
              <option value="any">All modes</option>
              <option value="locked">locked</option>
              <option value="unlocked">unlocked</option>
              <option value="unlocked_until">unlocked until</option>
              <option value="per_call_approval">per-call approval</option>
            </select>
            <button className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100">
              Filter wrappers
            </button>
            <input type="hidden" name="resource" value={selectedResource} />
            <input type="hidden" name="host" value={filters.host ?? ''} />
            <input type="hidden" name="user" value={filters.user ?? ''} />
            <input type="hidden" name="matrix_wrapper" value={filters.matrix_wrapper ?? ''} />
          </form>

          <div className="grid gap-4">
            {wrappersResponse.items.length > 0 ? (
              wrappersResponse.items.map((item) => (
                <WrapperRow
                  key={item.wrapper}
                  item={item}
                  store={store}
                  csrfToken={session.csrfToken}
                  query={query}
                />
              ))
            ) : (
              <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
                No wrappers matched the current filter.
              </div>
            )}
          </div>
        </Card>

        <Card title="Access matrix" eyebrow="Policy toggles">
          <form className="mb-5 grid gap-3 lg:grid-cols-[minmax(220px,0.9fr)_minmax(180px,0.7fr)_minmax(180px,0.7fr)_minmax(220px,0.9fr)_auto]" method="get">
            <select
              name="resource"
              defaultValue={selectedResource}
              className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm text-white focus:border-cyan-400/40 focus:outline-none"
            >
              {resourcesResponse.items.map((resource) => (
                <option key={resource.resource} value={resource.resource}>
                  {resource.resource}
                </option>
              ))}
            </select>
            <input
              name="host"
              defaultValue={filters.host ?? ''}
              placeholder="Host"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
            <input
              name="user"
              defaultValue={filters.user ?? ''}
              placeholder="User"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
            <input
              name="matrix_wrapper"
              defaultValue={filters.matrix_wrapper ?? ''}
              placeholder="Wrapper"
              className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
            />
            <button className="rounded-2xl border border-fuchsia-400/30 bg-fuchsia-400/10 px-4 py-3 text-sm font-medium text-fuchsia-100">
              Filter matrix
            </button>
            <input type="hidden" name="q" value={filters.q ?? ''} />
            <input type="hidden" name="mode" value={filters.mode ?? 'any'} />
          </form>

          <p className="mb-4 text-sm text-[var(--muted)]">
            Rows come from frontend catalog registrations. Matrix toggles apply policy for the selected wrapper + resource, so repeated rows with the same wrapper will reflect the same rule state.
          </p>

          {!selectedResource ? (
            <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
              No resources are available for this store yet.
            </div>
          ) : !matrixResponse ? (
            <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
              The selected matrix view could not be loaded.
            </div>
          ) : matrixResponse.rows.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
              No catalog rows matched the current matrix filter.
            </div>
          ) : (
            <div className="overflow-hidden rounded-2xl border border-white/10">
              <table className="min-w-full divide-y divide-white/10 text-left text-sm">
                <thead className="bg-black/30 text-[var(--muted)]">
                  <tr>
                    <th className="px-4 py-3 font-medium">Host</th>
                    <th className="px-4 py-3 font-medium">User</th>
                    <th className="px-4 py-3 font-medium">Wrapper</th>
                    {matrixResponse.operations.map((operation) => (
                      <th key={operation} className="px-4 py-3 font-medium">{operation}</th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/10 bg-black/10">
                  {matrixResponse.rows.map((row, index) => (
                    <tr key={`${row.host}:${row.user}:${row.wrapper}:${index}`}>
                      <td className="px-4 py-4 text-white">{row.host}</td>
                      <td className="px-4 py-4 text-white">{row.user}</td>
                      <td className="px-4 py-4 text-white">{row.wrapper}</td>
                      {matrixResponse.operations.map((operation) => (
                        <td key={operation} className="px-4 py-4">
                          <MatrixCellForm
                            store={store}
                            resource={selectedResource}
                            row={row}
                            operation={operation}
                            enabled={Boolean(row.cells[operation])}
                            csrfToken={session.csrfToken}
                            query={query}
                          />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </Shell>
  );
}
