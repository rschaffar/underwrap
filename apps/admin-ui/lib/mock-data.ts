export type NavSection = 'overview' | 'resources' | 'wrappers' | 'audit';

export type StoreSummary = {
  store: string;
  resourceCount: number;
  wrapperCounts: {
    locked: number;
    unlocked: number;
    perCallApproval: number;
  };
  activeMutationWindows: number;
  recentAuditEvents: number;
};

export type TargetSummary = {
  slug: string;
  label: string;
  status: 'reachable' | 'degraded';
  stores: StoreSummary[];
};

export const mockTargets: TargetSummary[] = [
  {
    slug: 'corp',
    label: 'corp',
    status: 'reachable',
    stores: [
      {
        store: 'work',
        resourceCount: 128,
        wrapperCounts: { locked: 2, unlocked: 14, perCallApproval: 3 },
        activeMutationWindows: 4,
        recentAuditEvents: 18,
      },
      {
        store: 'prod',
        resourceCount: 46,
        wrapperCounts: { locked: 4, unlocked: 6, perCallApproval: 5 },
        activeMutationWindows: 1,
        recentAuditEvents: 7,
      },
    ],
  },
  {
    slug: 'personal',
    label: 'personal',
    status: 'degraded',
    stores: [
      {
        store: 'default',
        resourceCount: 24,
        wrapperCounts: { locked: 1, unlocked: 5, perCallApproval: 1 },
        activeMutationWindows: 0,
        recentAuditEvents: 5,
      },
    ],
  },
];

export function getTargets() {
  return mockTargets;
}

export function getTarget(target: string) {
  return mockTargets.find((entry) => entry.slug === target) ?? mockTargets[0];
}

export function getStore(target: string, store: string) {
  return getTarget(target)?.stores.find((entry) => entry.store === store);
}

export function buildResourcePath(resource: string[]) {
  return resource.join('/');
}
