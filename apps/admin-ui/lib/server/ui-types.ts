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
