import { getConfiguredBackend } from './config.ts';
import type {
  UiAuditQueryResponse,
  UiMatrixQueryResponse,
  UiMutationGetResponse,
  UiOverviewResponse,
  UiResourceGetResponse,
  UiResourceRevealResponse,
  UiResourcesQueryResponse,
  UiWrappersQueryResponse,
} from './ui-types.ts';

export class BackendApiError extends Error {
  constructor(
    public readonly backendLabel: string,
    public readonly endpoint: string,
    public readonly status: number,
    message?: string,
  ) {
    super(message ?? `backend ${backendLabel} returned ${status} for ${endpoint}`);
  }
}

export class BackendClient {
  constructor(
    public readonly backendLabel: string,
    public readonly adminUrl: string,
    private readonly token: string,
  ) {}

  private async request(endpoint: string, init?: RequestInit): Promise<Response> {
    return fetch(`${this.adminUrl}${endpoint}`, {
      ...init,
      headers: {
        Authorization: `Bearer ${this.token}`,
        ...(init?.headers ?? {}),
      },
      cache: 'no-store',
    }).catch((error: unknown) => {
      throw new Error(`request to backend ${this.backendLabel} failed: ${String(error)}`);
    });
  }

  private async decodeJson<T>(response: Response, endpoint: string): Promise<T> {
    if (!response.ok) {
      throw new BackendApiError(this.backendLabel, endpoint, response.status);
    }
    return (await response.json()) as T;
  }

  async getJson<T>(endpoint: string): Promise<T> {
    const response = await this.request(endpoint);
    return this.decodeJson<T>(response, endpoint);
  }

  async postJson<T>(endpoint: string, body: unknown): Promise<T> {
    const response = await this.request(endpoint, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
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

  async revealResource(store: string, secretId: string): Promise<UiResourceRevealResponse> {
    return this.postJson('/v1/admin/ui/resource/reveal', {
      store,
      secret_id: secretId,
    });
  }

  async downloadResource(store: string, secretId: string): Promise<Response> {
    const response = await this.request('/v1/admin/ui/resource/download', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        store,
        secret_id: secretId,
      }),
    });
    if (!response.ok) {
      throw new BackendApiError(this.backendLabel, '/v1/admin/ui/resource/download', response.status);
    }
    return response;
  }

  async getMutationState(store: string, resource: string): Promise<UiMutationGetResponse> {
    return this.postJson('/v1/admin/ui/mutation/get', {
      store,
      resource,
    });
  }

  async setMutation(store: string, wrapper: string, secretId: string, mode: 'temporary' | 'disabled', expiresAtEpoch?: number) {
    return this.postJson('/v1/admin/ui/mutation/set', {
      store,
      wrapper,
      secret_id: secretId,
      mode,
      expires_at_epoch: expiresAtEpoch ?? null,
    });
  }

  async writeTextResource(store: string, wrapper: string, secretId: string, materialUtf8: string) {
    return this.postJson('/v1/admin/ui/resource/write-text', {
      store,
      wrapper,
      secret_id: secretId,
      material_utf8: materialUtf8,
    });
  }

  async uploadFileResource(store: string, wrapper: string, secretId: string, materialBase64: string) {
    return this.postJson('/v1/admin/ui/resource/upload-file', {
      store,
      wrapper,
      secret_id: secretId,
      material_b64: materialBase64,
    });
  }

  async deleteResourceEntry(store: string, wrapper: string, secretId: string) {
    return this.postJson('/v1/admin/ui/resource/delete-entry', {
      store,
      wrapper,
      secret_id: secretId,
    });
  }

  async queryWrappers(store: string, query: { q?: string; mode?: string }): Promise<UiWrappersQueryResponse> {
    return this.postJson('/v1/admin/ui/wrappers/query', {
      store,
      query: query.q ?? '',
      mode: query.mode && query.mode !== 'any' ? query.mode : null,
    });
  }

  async setWrapperMode(store: string, wrapper: string, mode: Record<string, unknown>, reason?: string) {
    return this.postJson('/v1/admin/ui/wrapper/set-mode', {
      store,
      wrapper,
      mode,
      reason: reason?.trim() ? reason.trim() : null,
    });
  }

  async queryMatrix(
    store: string,
    query: { resource?: string; host?: string; user?: string; wrapper?: string },
  ): Promise<UiMatrixQueryResponse> {
    return this.postJson('/v1/admin/ui/matrix/query', {
      store,
      resource: query.resource?.trim() ? query.resource.trim() : null,
      host: query.host?.trim() ? query.host.trim() : null,
      user: query.user?.trim() ? query.user.trim() : null,
      wrapper: query.wrapper?.trim() ? query.wrapper.trim() : null,
    });
  }

  async setMatrixCell(store: string, wrapper: string, resource: string, operation: string, enabled: boolean) {
    return this.postJson('/v1/admin/ui/matrix/set-cell', {
      store,
      wrapper,
      resource,
      operation,
      enabled,
    });
  }

  async queryAudit(
    store: string,
    query: {
      operator?: string;
      wrapper?: string;
      resource?: string;
      action?: string;
      from_epoch?: number | null;
      to_epoch?: number | null;
    },
  ): Promise<UiAuditQueryResponse> {
    return this.postJson('/v1/admin/ui/audit/query', {
      store,
      operator: query.operator?.trim() ? query.operator.trim() : null,
      wrapper: query.wrapper?.trim() ? query.wrapper.trim() : null,
      resource: query.resource?.trim() ? query.resource.trim() : null,
      action: query.action?.trim() && query.action !== 'any' ? query.action.trim() : null,
      from_epoch: query.from_epoch ?? null,
      to_epoch: query.to_epoch ?? null,
      page: 1,
      page_size: 200,
    });
  }
}

export async function getConfiguredBackendClient() {
  const backend = await getConfiguredBackend();
  return new BackendClient(backend.label, backend.adminUrl, backend.token);
}
