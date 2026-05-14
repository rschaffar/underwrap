import { getConfiguredBackend, listConfiguredTargets } from './config.ts';
import type {
  UiOverviewResponse,
  UiResourceGetResponse,
  UiResourcesQueryResponse,
} from './ui-types.ts';

export class BackendApiError extends Error {
  constructor(
    public readonly target: string,
    public readonly endpoint: string,
    public readonly status: number,
    message?: string,
  ) {
    super(message ?? `backend target ${target} returned ${status} for ${endpoint}`);
  }
}

export class BackendClient {
  constructor(
    public readonly target: string,
    public readonly adminUrl: string,
    private readonly token: string,
  ) {}

  private async decodeJson<T>(response: Response, endpoint: string): Promise<T> {
    if (!response.ok) {
      throw new BackendApiError(this.target, endpoint, response.status);
    }
    return (await response.json()) as T;
  }

  async getJson<T>(endpoint: string): Promise<T> {
    const response = await fetch(`${this.adminUrl}${endpoint}`, {
      headers: {
        Authorization: `Bearer ${this.token}`,
      },
      cache: 'no-store',
    }).catch((error: unknown) => {
      throw new Error(`request to backend target ${this.target} failed: ${String(error)}`);
    });

    return this.decodeJson<T>(response, endpoint);
  }

  async postJson<T>(endpoint: string, body: unknown): Promise<T> {
    const response = await fetch(`${this.adminUrl}${endpoint}`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
      cache: 'no-store',
    }).catch((error: unknown) => {
      throw new Error(`request to backend target ${this.target} failed: ${String(error)}`);
    });

    return this.decodeJson<T>(response, endpoint);
  }

  async listStores(): Promise<string[]> {
    const body = await this.getJson<{ stores: string[] }>('/v1/admin/stores');
    return body.stores;
  }

  async getOverview(): Promise<UiOverviewResponse> {
    return this.getJson('/v1/admin/ui/overview');
  }

  async queryResources(store: string, query: { q?: string; slot?: string }): Promise<UiResourcesQueryResponse> {
    return this.postJson('/v1/admin/ui/resources/query', {
      store,
      query: query.q ?? '',
      slot: query.slot ?? null,
      mutation_state: 'any',
      page: 1,
      page_size: 100,
    });
  }

  async getResourceDetail(store: string, resource: string): Promise<UiResourceGetResponse> {
    return this.postJson('/v1/admin/ui/resource/get', {
      store,
      resource,
    });
  }
}

export async function getBackendClient(target: string) {
  const backend = await getConfiguredBackend(target);
  if (!backend) {
    return null;
  }
  return new BackendClient(target, backend.adminUrl, backend.token);
}

export async function getConfiguredBackendClients(): Promise<BackendClient[]> {
  const targets = await listConfiguredTargets();
  const clients = await Promise.all(
    targets.map(async (target) => {
      const backend = await getConfiguredBackend(target.name);
      if (!backend) {
        return null;
      }
      return new BackendClient(target.name, backend.adminUrl, backend.token);
    }),
  );
  return clients.filter((client): client is BackendClient => client !== null);
}
