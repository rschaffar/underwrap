export type AuditFilterParams = {
  operator?: string;
  wrapper?: string;
  resource?: string;
  action?: string;
  from_epoch?: string;
  to_epoch?: string;
};

export function parseEpochSearchParam(value: string | undefined) {
  if (!value) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : null;
}

export function epochToDateTimeLocal(epoch?: number | null) {
  if (!epoch) {
    return '';
  }
  const date = new Date(epoch * 1000);
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 16);
}

export function auditActionTone(action: string) {
  if (action === 'secret_revealed' || action === 'secret_downloaded') {
    return 'text-cyan-100';
  }
  if (action.startsWith('mutation_')) {
    return 'text-amber-200';
  }
  if (action === 'secret_deleted') {
    return 'text-rose-200';
  }
  if (action === 'wrapper_mode_updated' || action.startsWith('matrix_cell_')) {
    return 'text-fuchsia-200';
  }
  return 'text-emerald-200';
}

export function buildAuditQuery(params: AuditFilterParams) {
  return {
    operator: params.operator?.trim() || undefined,
    wrapper: params.wrapper?.trim() || undefined,
    resource: params.resource?.trim() || undefined,
    action: params.action?.trim() && params.action !== 'any' ? params.action.trim() : undefined,
    from_epoch: parseEpochSearchParam(params.from_epoch),
    to_epoch: parseEpochSearchParam(params.to_epoch),
  };
}
