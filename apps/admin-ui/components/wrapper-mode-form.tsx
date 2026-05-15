'use client';

import { useMemo, useState } from 'react';

import type { UiWrapperApprovalMode } from '../lib/server/ui-types';

type WrapperModeFormProps = {
  store: string;
  wrapper: string;
  csrfToken: string;
  currentMode: UiWrapperApprovalMode;
  currentReason?: string | null;
  query: {
    q?: string;
    mode?: string;
    resource?: string;
    host?: string;
    user?: string;
    matrix_wrapper?: string;
  };
};

type ModeKind = 'locked' | 'unlocked' | 'unlocked_until' | 'per_call_approval';

function modeKindFromMode(mode: UiWrapperApprovalMode): ModeKind {
  if (mode.kind === 'locked') {
    return 'locked';
  }
  if (mode.kind === 'per-call-approval') {
    return 'per_call_approval';
  }
  return mode.until_epoch ? 'unlocked_until' : 'unlocked';
}

function toDateTimeLocal(epoch?: number | null) {
  if (!epoch) {
    return '';
  }
  const date = new Date(epoch * 1000);
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 16);
}

export function WrapperModeForm({ store, wrapper, csrfToken, currentMode, currentReason, query }: WrapperModeFormProps) {
  const [modeKind, setModeKind] = useState<ModeKind>(modeKindFromMode(currentMode));
  const [untilLocal, setUntilLocal] = useState(
    currentMode.kind === 'unlocked' ? toDateTimeLocal(currentMode.until_epoch ?? null) : '',
  );
  const [approvalKey, setApprovalKey] = useState(
    currentMode.kind === 'per-call-approval' ? currentMode.approval_key : '',
  );
  const [reason, setReason] = useState(currentReason ?? '');

  const untilEpoch = useMemo(() => {
    if (modeKind !== 'unlocked_until' || !untilLocal) {
      return '';
    }
    const epoch = Math.floor(new Date(untilLocal).getTime() / 1000);
    return Number.isFinite(epoch) && epoch > 0 ? String(epoch) : '';
  }, [modeKind, untilLocal]);

  return (
    <form method="post" className="grid gap-3 rounded-2xl border border-white/10 bg-black/20 p-4">
      <input type="hidden" name="store" value={store} />
      <input type="hidden" name="wrapper" value={wrapper} />
      <input type="hidden" name="csrf_token" value={csrfToken} />
      <input type="hidden" name="until_epoch" value={untilEpoch} />
      <input type="hidden" name="q" value={query.q ?? ''} />
      <input type="hidden" name="mode" value={query.mode ?? ''} />
      <input type="hidden" name="resource" value={query.resource ?? ''} />
      <input type="hidden" name="host" value={query.host ?? ''} />
      <input type="hidden" name="user" value={query.user ?? ''} />
      <input type="hidden" name="matrix_wrapper" value={query.matrix_wrapper ?? ''} />

      <label className="grid gap-2 text-sm text-[var(--muted)]">
        Mode
        <select
          name="mode_kind"
          value={modeKind}
          onChange={(event) => setModeKind(event.target.value as ModeKind)}
          className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white focus:border-cyan-400/40 focus:outline-none"
        >
          <option value="locked">locked</option>
          <option value="unlocked">unlocked (permanent)</option>
          <option value="unlocked_until">unlocked until…</option>
          <option value="per_call_approval">per-call approval</option>
        </select>
      </label>

      {modeKind === 'unlocked_until' ? (
        <label className="grid gap-2 text-sm text-[var(--muted)]">
          Unlock until
          <input
            type="datetime-local"
            value={untilLocal}
            onChange={(event) => setUntilLocal(event.target.value)}
            className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white focus:border-cyan-400/40 focus:outline-none"
          />
        </label>
      ) : null}

      {modeKind === 'per_call_approval' ? (
        <label className="grid gap-2 text-sm text-[var(--muted)]">
          Approval key
          <textarea
            name="approval_key"
            rows={3}
            value={approvalKey}
            onChange={(event) => setApprovalKey(event.target.value)}
            className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 font-mono text-sm text-white focus:border-cyan-400/40 focus:outline-none"
          />
        </label>
      ) : (
        <input type="hidden" name="approval_key" value="" />
      )}

      <label className="grid gap-2 text-sm text-[var(--muted)]">
        Reason
        <input
          name="reason"
          value={reason}
          onChange={(event) => setReason(event.target.value)}
          className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white focus:border-cyan-400/40 focus:outline-none"
        />
      </label>

      <div className="flex flex-wrap items-center gap-3">
        <button
          type="submit"
          formAction="/actions/wrappers/set-mode"
          className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100"
        >
          Save mode
        </button>
        {modeKind === 'unlocked_until' ? (
          <span className="text-xs text-[var(--muted)]">The timestamp is converted to epoch seconds in the browser before submit.</span>
        ) : null}
      </div>
    </form>
  );
}
