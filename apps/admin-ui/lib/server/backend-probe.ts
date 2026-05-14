import { getConfiguredBackendClients } from './backend-client';

export type BackendProbe = {
  name: string;
  adminUrl: string;
  reachable: boolean;
  storeCount: number | null;
  error: string | null;
};

export async function probeConfiguredBackends(): Promise<BackendProbe[]> {
  const clients = await getConfiguredBackendClients();

  return Promise.all(
    clients.map(async (client) => {
      try {
        const stores = await client.listStores();
        return {
          name: client.target,
          adminUrl: client.adminUrl,
          reachable: true,
          storeCount: stores.length,
          error: null,
        } satisfies BackendProbe;
      } catch {
        return {
          name: client.target,
          adminUrl: client.adminUrl,
          reachable: false,
          storeCount: null,
          error: 'Backend unreachable or not yet configured for this environment.',
        } satisfies BackendProbe;
      }
    }),
  );
}
