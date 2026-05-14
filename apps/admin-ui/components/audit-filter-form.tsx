'use client';

import { useMemo, useState } from 'react';

import { epochToDateTimeLocal } from '../lib/server/audit-filters.ts';

type AuditFilterFormProps = {
  defaultValues: {
    operator?: string;
    wrapper?: string;
    resource?: string;
    action?: string;
    from_epoch?: number | null;
    to_epoch?: number | null;
  };
};

export function AuditFilterForm({ defaultValues }: AuditFilterFormProps) {
  const [fromLocal, setFromLocal] = useState(epochToDateTimeLocal(defaultValues.from_epoch));
  const [toLocal, setToLocal] = useState(epochToDateTimeLocal(defaultValues.to_epoch));

  const fromEpoch = useMemo(() => {
    if (!fromLocal) {
      return '';
    }
    const epoch = Math.floor(new Date(fromLocal).getTime() / 1000);
    return Number.isFinite(epoch) && epoch > 0 ? String(epoch) : '';
  }, [fromLocal]);

  const toEpoch = useMemo(() => {
    if (!toLocal) {
      return '';
    }
    const epoch = Math.floor(new Date(toLocal).getTime() / 1000);
    return Number.isFinite(epoch) && epoch > 0 ? String(epoch) : '';
  }, [toLocal]);

  return (
    <form method="get" className="grid gap-3 lg:grid-cols-[repeat(4,minmax(0,1fr))_minmax(0,1fr)_minmax(0,1fr)_auto]">
      <input type="hidden" name="from_epoch" value={fromEpoch} />
      <input type="hidden" name="to_epoch" value={toEpoch} />
      <input
        name="operator"
        defaultValue={defaultValues.operator ?? ''}
        placeholder="Operator"
        className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
      />
      <input
        name="wrapper"
        defaultValue={defaultValues.wrapper ?? ''}
        placeholder="Wrapper"
        className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
      />
      <input
        name="resource"
        defaultValue={defaultValues.resource ?? ''}
        placeholder="Resource"
        className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white placeholder:text-[var(--muted)] focus:border-cyan-400/40 focus:outline-none"
      />
      <select
        name="action"
        defaultValue={defaultValues.action ?? 'any'}
        className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm text-white focus:border-cyan-400/40 focus:outline-none"
      >
        <option value="any">All actions</option>
        <option value="secret_revealed">secret_revealed</option>
        <option value="secret_downloaded">secret_downloaded</option>
        <option value="secret_stored">secret_stored</option>
        <option value="secret_deleted">secret_deleted</option>
        <option value="mutation_enabled">mutation_enabled</option>
        <option value="mutation_disabled">mutation_disabled</option>
        <option value="wrapper_mode_updated">wrapper_mode_updated</option>
        <option value="matrix_cell_enabled">matrix_cell_enabled</option>
        <option value="matrix_cell_disabled">matrix_cell_disabled</option>
      </select>
      <input
        type="datetime-local"
        value={fromLocal}
        onChange={(event) => setFromLocal(event.target.value)}
        className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white focus:border-cyan-400/40 focus:outline-none"
      />
      <input
        type="datetime-local"
        value={toLocal}
        onChange={(event) => setToLocal(event.target.value)}
        className="rounded-2xl border border-white/10 bg-black/20 px-4 py-3 text-white focus:border-cyan-400/40 focus:outline-none"
      />
      <button className="rounded-2xl border border-cyan-400/30 bg-cyan-400/10 px-4 py-3 text-sm font-medium text-cyan-100">
        Filter audit
      </button>
    </form>
  );
}
