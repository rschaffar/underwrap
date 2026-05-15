import { getConfiguredBackendClient } from './backend-client.ts';

export async function loadNavigation() {
  const client = await getConfiguredBackendClient();

  try {
    const overview = await client.getOverview();
    return {
      backend: {
        label: client.backendLabel,
        status: 'reachable' as const,
      },
      stores: overview.stores.map((store) => store.store),
    };
  } catch {
    return {
      backend: {
        label: client.backendLabel,
        status: 'degraded' as const,
      },
      stores: [] as string[],
    };
  }
}

export async function requireBackendClient() {
  return getConfiguredBackendClient();
}
