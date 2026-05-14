import { getConfiguredBackendClients, getBackendClient } from './backend-client.ts';

export type NavigationTarget = {
  slug: string;
  label: string;
  status: 'reachable' | 'degraded';
  stores: string[];
};

export async function loadNavigation(selectedTarget?: string) {
  const clients = await getConfiguredBackendClients();
  const targets = await Promise.all(
    clients.map(async (client) => {
      try {
        const overview = await client.getOverview();
        return {
          slug: client.target,
          label: client.target,
          status: 'reachable' as const,
          stores: overview.stores.map((store) => store.store),
        };
      } catch {
        return {
          slug: client.target,
          label: client.target,
          status: 'degraded' as const,
          stores: [],
        };
      }
    }),
  );

  const activeTarget = selectedTarget
    ? targets.find((target) => target.slug === selectedTarget)
    : undefined;

  return {
    targets,
    activeTarget,
  };
}

export async function requireBackendClient(target: string) {
  const client = await getBackendClient(target);
  if (!client) {
    return null;
  }
  return client;
}
