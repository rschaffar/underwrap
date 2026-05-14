import { ReactNode } from 'react';

import type { UiResourceEntrySummary } from '../lib/server/ui-types';

type ResourceEntryTableProps = {
  title: string;
  entries: UiResourceEntrySummary[];
  emptyLabel: string;
  actions: (entry: UiResourceEntrySummary) => ReactNode;
};

function formatBytes(size: number) {
  if (size < 1024) {
    return `${size} B`;
  }
  const kib = size / 1024;
  if (kib < 1024) {
    return `${kib.toFixed(1)} KiB`;
  }
  return `${(kib / 1024).toFixed(1)} MiB`;
}

export function ResourceEntryTable({ title, entries, emptyLabel, actions }: ResourceEntryTableProps) {
  return (
    <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
      <div className="mb-4 flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold uppercase tracking-[0.2em] text-white">{title}</h3>
        <div className="text-xs text-[var(--muted)]">{entries.length} entries</div>
      </div>
      {entries.length === 0 ? (
        <div className="rounded-2xl border border-dashed border-white/10 px-4 py-6 text-sm text-[var(--muted)]">
          {emptyLabel}
        </div>
      ) : (
        <div className="overflow-hidden rounded-2xl border border-white/10">
          <table className="min-w-full divide-y divide-white/10 text-left text-sm">
            <thead className="bg-black/30 text-[var(--muted)]">
              <tr>
                <th className="px-4 py-3 font-medium">Entry</th>
                <th className="px-4 py-3 font-medium">Type</th>
                <th className="px-4 py-3 font-medium">Size</th>
                <th className="px-4 py-3 font-medium">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10 bg-black/10">
              {entries.map((entry) => (
                <tr key={entry.secret_id}>
                  <td className="px-4 py-4 text-white">{entry.entry}</td>
                  <td className="px-4 py-4 text-[var(--muted)]">{entry.is_utf8 ? 'text' : 'binary'}</td>
                  <td className="px-4 py-4 text-[var(--muted)]">{formatBytes(entry.size_bytes)}</td>
                  <td className="px-4 py-4">{actions(entry)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
