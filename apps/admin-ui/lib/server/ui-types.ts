export type UiOverviewStore = {
  store: string;
  resource_count: number;
  wrapper_counts: {
    locked: number;
    unlocked: number;
    unlocked_until: number;
    per_call_approval: number;
  };
  active_mutation_windows: number;
  recent_audit_events: number;
};

export type UiOverviewResponse = {
  stores: UiOverviewStore[];
};

export type UiResourceListItem = {
  resource: string;
  slots: {
    env: boolean;
    files: boolean;
    libsecret: boolean;
  };
  catalog_wrappers: string[];
  mutation: {
    active: boolean;
  };
};

export type UiResourcesQueryResponse = {
  items: UiResourceListItem[];
  total: number;
};

export type UiResourceEntrySummary = {
  entry: string;
  secret_id: string;
  size_bytes: number;
  is_utf8: boolean;
  sha256_hex?: string | null;
};

export type UiResourceGetResponse = {
  resource: string;
  slots: {
    env: UiResourceEntrySummary[];
    files: UiResourceEntrySummary[];
    libsecret: UiResourceEntrySummary[];
  };
};

export type UiResourceRevealResponse =
  | {
      kind: 'utf8';
      text: string;
      size_bytes: number;
      copy_allowed: boolean;
    }
  | {
      kind: 'binary';
      base64: string;
      size_bytes: number;
      copy_allowed: boolean;
      download_recommended: boolean;
    };

export type UiMutationWrapperState = {
  wrapper: string;
  active: boolean;
  expires_at_epoch?: number | null;
  mode: string;
};

export type UiMutationGetResponse = {
  wrappers: UiMutationWrapperState[];
};

export type UiWrapperApprovalMode =
  | {
      kind: 'locked';
    }
  | {
      kind: 'unlocked';
      until_epoch?: number | null;
    }
  | {
      kind: 'per-call-approval';
      approval_key: string;
    };

export type UiWrapperListItem = {
  wrapper: string;
  mode: UiWrapperApprovalMode;
  reason?: string | null;
  resource_count: number;
};

export type UiWrappersQueryResponse = {
  items: UiWrapperListItem[];
};

export type UiMatrixRow = {
  host: string;
  user: string;
  wrapper: string;
  cells: Record<string, boolean>;
};

export type UiMatrixQueryResponse = {
  operations: string[];
  rows: UiMatrixRow[];
};

export type UiAuditEvent = {
  ts_epoch: number;
  action: string;
  operator: string;
  wrapper?: string | null;
  resource?: string | null;
  secret_id?: string | null;
  operation?: string | null;
  result: string;
};

export type UiAuditQueryResponse = {
  items: UiAuditEvent[];
  total: number;
};
