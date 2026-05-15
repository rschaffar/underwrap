import { getConfiguredBackendClient } from './backend-client.ts';

export type BackendProbe = {
  name: string;
  adminUrl: string;
  reachable: boolean;
  storeCount: number | null;
  error: string | null;
};

export async function probeConfiguredBackends(): Promise<BackendProbe[]> {
  const client = await getConfiguredBackendClient();

  try {
    const stores = await client.listStores();
    return [
      {
        name: client.backendLabel,
        adminUrl: client.adminUrl,
        reachable: true,
        storeCount: stores.length,
        error: null,
      } satisfies BackendProbe,
    ];
  } catch {
    return [
      {
        name: client.backendLabel,
        adminUrl: client.adminUrl,
        reachable: false,
        storeCount: null,
        error: 'Backend unreachable or not yet configured for this environment.',
      } satisfies BackendProbe,
    ];
  }
}
