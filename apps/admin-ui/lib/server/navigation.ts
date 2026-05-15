import { getConfiguredBackendClient } from './backend-client.ts';

export type NavigationTarget = {
  slug: string;
  label: string;
  status: 'reachable' | 'degraded';
  stores: string[];
};

export async function loadNavigation(selectedTarget?: string) {
  const client = await getConfiguredBackendClient();

  const backend = await (async () => {
    try {
      const overview = await client.getOverview();
      return {
        slug: client.backendLabel,
        label: client.backendLabel,
        status: 'reachable' as const,
        stores: overview.stores.map((store) => store.store),
      };
    } catch {
      return {
        slug: client.backendLabel,
        label: client.backendLabel,
        status: 'degraded' as const,
        stores: [],
      };
    }
  })();

  const activeTarget = !selectedTarget || selectedTarget === backend.slug ? backend : undefined;

  return {
    targets: [backend],
    activeTarget,
  };
}

export async function requireBackendClient(target?: string) {
  const client = await getConfiguredBackendClient();
  if (target && target !== client.backendLabel) {
    return null;
  }
  return client;
}
