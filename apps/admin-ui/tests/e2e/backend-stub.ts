import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const TOKEN_PATH = path.join(process.cwd(), 'dev-fixtures', 'corp.token');

type WrapperMode =
  | { kind: 'locked' }
  | { kind: 'unlocked'; until_epoch?: number | null }
  | { kind: 'per-call-approval'; approval_key: string };

type AuditEvent = {
  ts_epoch: number;
  action: string;
  operator: string;
  wrapper?: string | null;
  resource?: string | null;
  secret_id?: string | null;
  operation?: string | null;
  result: string;
};

type State = {
  wrappers: Array<{ wrapper: string; mode: WrapperMode; reason?: string | null; resource_count: number }>;
  matrix: Record<string, Record<string, boolean>>;
  mutation: Record<string, Record<string, { active: boolean; mode: string; expires_at_epoch?: number | null }>>;
  secrets: Record<string, Buffer>;
  audit: AuditEvent[];
};

const INITIAL_WRAPPERS: State['wrappers'] = [
  {
    wrapper: 'gh-authorized',
    mode: { kind: 'unlocked' },
    reason: 'default ops mode',
    resource_count: 1,
  },
  {
    wrapper: 'jira-readonly',
    mode: { kind: 'locked' },
    reason: 'change freeze',
    resource_count: 1,
  },
];

const INITIAL_MATRIX: State['matrix'] = {
  'gh-authorized|gh/main': {
    'env.read': true,
    'files.read': true,
    'files.write': false,
    'libsecret.read': true,
    'libsecret.write': false,
  },
  'jira-readonly|gh/main': {
    'env.read': false,
    'files.read': false,
    'files.write': false,
    'libsecret.read': false,
    'libsecret.write': false,
  },
};

function initialState(): State {
  return {
    wrappers: structuredClone(INITIAL_WRAPPERS),
    matrix: structuredClone(INITIAL_MATRIX),
    mutation: {
      'gh/main': {
        'gh-authorized': { active: false, mode: 'disabled', expires_at_epoch: null },
        'jira-readonly': { active: false, mode: 'disabled', expires_at_epoch: null },
      },
    },
    secrets: {
      'gh/main/env/GH_TOKEN': Buffer.from('ghp_e2e_secret'),
      'gh/main/files/hosts.json': Buffer.from('{"host":"example"}'),
      'gh/main/files/blob.bin': Buffer.from([0, 1, 2, 3]),
      'gh/main/libsecret/default/item_1': Buffer.from('serialized-item'),
    },
    audit: [
      {
        ts_epoch: 1760000000,
        action: 'wrapper_mode_updated',
        operator: 'alice',
        wrapper: 'gh-authorized',
        resource: null,
        secret_id: null,
        operation: null,
        result: 'success',
      },
    ],
  };
}

function json(response: ServerResponse, status: number, body: unknown, extraHeaders: Record<string, string> = {}) {
  response.writeHead(status, {
    'Content-Type': 'application/json',
    'Cache-Control': 'no-store',
    ...extraHeaders,
  });
  response.end(JSON.stringify(body));
}

function notFound(response: ServerResponse) {
  json(response, 404, { code: 'not_found', message: 'not found' });
}

async function readJson(request: IncomingMessage) {
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  if (chunks.length === 0) {
    return {};
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
}

function splitSecretId(secretId: string) {
  for (const slot of ['env', 'files', 'libsecret'] as const) {
    const marker = `/${slot}/`;
    const index = secretId.indexOf(marker);
    if (index !== -1) {
      return { resource: secretId.slice(0, index), slot, entry: secretId.slice(index + marker.length) };
    }
  }
  throw new Error(`invalid secret id ${secretId}`);
}

function listResourceEntries(state: State, resource: string, slot: 'env' | 'files' | 'libsecret') {
  return Object.entries(state.secrets)
    .filter(([secretId]) => secretId.startsWith(`${resource}/${slot}/`))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([secretId, material]) => ({
      entry: splitSecretId(secretId).entry,
      secret_id: secretId,
      size_bytes: material.length,
      is_utf8: !material.includes(0),
      sha256_hex: slot === 'files' ? 'stubbed-sha256' : null,
    }));
}

function mutationStateFor(state: State, resource: string) {
  const wrappers = state.mutation[resource] ?? {};
  return {
    wrappers: Object.entries(wrappers)
      .map(([wrapper, details]) => ({ wrapper, ...details }))
      .sort((a, b) => a.wrapper.localeCompare(b.wrapper)),
  };
}

function pushAudit(state: State, event: Omit<AuditEvent, 'ts_epoch' | 'operator' | 'result'> & Partial<Pick<AuditEvent, 'operator' | 'result'>>) {
  state.audit.push({
    ts_epoch: Math.floor(Date.now() / 1000),
    operator: event.operator ?? 'alice',
    result: event.result ?? 'success',
    ...event,
  });
}

function requireAuthorized(request: IncomingMessage, response: ServerResponse, token: string) {
  const auth = request.headers.authorization ?? '';
  if (auth !== `Bearer ${token}`) {
    json(response, 401, { code: 'unauthorized', message: 'missing or invalid bearer token' });
    return false;
  }
  return true;
}

export async function startBackendStub(port: number) {
  const token = (await readFile(TOKEN_PATH, 'utf8')).trim();
  let state = initialState();

  const server = createServer(async (request, response) => {
    if (!request.url || !request.method) {
      return notFound(response);
    }

    if (request.url === '/__test/reset' && request.method === 'POST') {
      state = initialState();
      return json(response, 200, { ok: true });
    }

    if (!requireAuthorized(request, response, token)) {
      return;
    }

    if (request.url === '/v1/admin/ui/overview' && request.method === 'GET') {
      const resourceCount = new Set(Object.keys(state.secrets).map((secretId) => splitSecretId(secretId).resource)).size;
      return json(response, 200, {
        stores: [
          {
            store: 'work',
            resource_count: resourceCount,
            wrapper_counts: {
              locked: state.wrappers.filter((entry) => entry.mode.kind === 'locked').length,
              unlocked: state.wrappers.filter((entry) => entry.mode.kind === 'unlocked' && !entry.mode.until_epoch).length,
              unlocked_until: state.wrappers.filter((entry) => entry.mode.kind === 'unlocked' && entry.mode.until_epoch).length,
              per_call_approval: state.wrappers.filter((entry) => entry.mode.kind === 'per-call-approval').length,
            },
            active_mutation_windows: Object.values(state.mutation['gh/main'] ?? {}).filter((entry) => entry.active).length,
            recent_audit_events: state.audit.length,
          },
        ],
      });
    }

    const body = await readJson(request);

    if (request.url === '/v1/admin/ui/resources/query' && request.method === 'POST') {
      const resource = 'gh/main';
      return json(response, 200, {
        items: [
          {
            resource,
            slots: {
              env: listResourceEntries(state, resource, 'env').length > 0,
              files: listResourceEntries(state, resource, 'files').length > 0,
              libsecret: listResourceEntries(state, resource, 'libsecret').length > 0,
            },
            catalog_wrappers: ['gh-authorized', 'jira-readonly'],
            mutation: { active: Object.values(state.mutation[resource] ?? {}).some((entry) => entry.active) },
          },
        ],
        total: 1,
      });
    }

    if (request.url === '/v1/admin/ui/resource/get' && request.method === 'POST') {
      const resource = String(body.resource ?? 'gh/main');
      return json(response, 200, {
        resource,
        slots: {
          env: listResourceEntries(state, resource, 'env'),
          files: listResourceEntries(state, resource, 'files'),
          libsecret: listResourceEntries(state, resource, 'libsecret'),
        },
      });
    }

    if (request.url === '/v1/admin/ui/resource/reveal' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const material = state.secrets[secretId];
      if (!material) {
        return json(response, 404, { code: 'not_found', message: 'unknown secret' });
      }
      pushAudit(state, {
        action: 'secret_revealed',
        resource: splitSecretId(secretId).resource,
        secret_id: secretId,
      });
      if (material.includes(0)) {
        return json(response, 200, {
          kind: 'binary',
          base64: material.toString('base64'),
          size_bytes: material.length,
          copy_allowed: false,
          download_recommended: true,
        });
      }
      return json(response, 200, {
        kind: 'utf8',
        text: material.toString('utf8'),
        size_bytes: material.length,
        copy_allowed: true,
      });
    }

    if (request.url === '/v1/admin/ui/resource/download' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const material = state.secrets[secretId];
      if (!material) {
        return json(response, 404, { code: 'not_found', message: 'unknown secret' });
      }
      pushAudit(state, {
        action: 'secret_downloaded',
        resource: splitSecretId(secretId).resource,
        secret_id: secretId,
      });
      response.writeHead(200, {
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': `attachment; filename="${splitSecretId(secretId).entry.split('/').pop()}"`,
        'Cache-Control': 'no-store',
      });
      response.end(material);
      return;
    }

    if (request.url === '/v1/admin/ui/mutation/get' && request.method === 'POST') {
      return json(response, 200, mutationStateFor(state, String(body.resource ?? 'gh/main')));
    }

    if (request.url === '/v1/admin/ui/mutation/set' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const resource = splitSecretId(secretId).resource;
      const wrapper = String(body.wrapper ?? '');
      const mode = String(body.mode ?? 'disabled');
      const active = mode === 'temporary';
      const expiresAt = active ? Number(body.expires_at_epoch ?? Math.floor(Date.now() / 1000) + 1800) : null;
      state.mutation[resource] ??= {};
      state.mutation[resource][wrapper] = {
        active,
        mode: active ? 'temporary' : 'disabled',
        expires_at_epoch: expiresAt,
      };
      pushAudit(state, {
        action: active ? 'mutation_enabled' : 'mutation_disabled',
        wrapper,
        resource,
        secret_id: secretId,
        operation: 'store.write',
      });
      return json(response, 200, {
        active,
        expires_at_epoch: expiresAt,
        mode: active ? 'temporary' : 'disabled',
      });
    }

    if (request.url === '/v1/admin/ui/resource/write-text' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const wrapper = String(body.wrapper ?? '');
      const resource = splitSecretId(secretId).resource;
      if (!state.mutation[resource]?.[wrapper]?.active) {
        return json(response, 403, { code: 'mutation_denied', message: 'mutation window not active' });
      }
      state.secrets[secretId] = Buffer.from(String(body.material_utf8 ?? ''), 'utf8');
      pushAudit(state, { action: 'secret_stored', wrapper, resource, secret_id: secretId });
      return json(response, 200, {
        entry: {
          entry: splitSecretId(secretId).entry,
          secret_id: secretId,
          size_bytes: state.secrets[secretId].length,
          is_utf8: true,
          sha256_hex: null,
        },
      });
    }

    if (request.url === '/v1/admin/ui/resource/upload-file' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const wrapper = String(body.wrapper ?? '');
      const resource = splitSecretId(secretId).resource;
      if (!state.mutation[resource]?.[wrapper]?.active) {
        return json(response, 403, { code: 'mutation_denied', message: 'mutation window not active' });
      }
      const material = Buffer.from(String(body.material_b64 ?? ''), 'base64');
      state.secrets[secretId] = material;
      pushAudit(state, { action: 'secret_stored', wrapper, resource, secret_id: secretId });
      return json(response, 200, {
        entry: {
          entry: splitSecretId(secretId).entry,
          secret_id: secretId,
          size_bytes: material.length,
          is_utf8: !material.includes(0),
          sha256_hex: 'stubbed-sha256',
        },
      });
    }

    if (request.url === '/v1/admin/ui/resource/delete-entry' && request.method === 'POST') {
      const secretId = String(body.secret_id ?? '');
      const wrapper = String(body.wrapper ?? '');
      const resource = splitSecretId(secretId).resource;
      if (!state.mutation[resource]?.[wrapper]?.active) {
        return json(response, 403, { code: 'mutation_denied', message: 'mutation window not active' });
      }
      const existed = secretId in state.secrets;
      delete state.secrets[secretId];
      if (existed) {
        pushAudit(state, { action: 'secret_deleted', wrapper, resource, secret_id: secretId });
      }
      return json(response, 200, { existed });
    }

    if (request.url === '/v1/admin/ui/wrappers/query' && request.method === 'POST') {
      const query = String(body.query ?? '').toLowerCase();
      const mode = body.mode == null ? 'any' : String(body.mode);
      const items = state.wrappers.filter((entry) => {
        if (query && !entry.wrapper.toLowerCase().includes(query)) {
          return false;
        }
        if (mode === 'any') {
          return true;
        }
        if (mode === 'locked') {
          return entry.mode.kind === 'locked';
        }
        if (mode === 'unlocked') {
          return entry.mode.kind === 'unlocked' && !entry.mode.until_epoch;
        }
        if (mode === 'unlocked_until') {
          return entry.mode.kind === 'unlocked' && Boolean(entry.mode.until_epoch);
        }
        if (mode === 'per_call_approval') {
          return entry.mode.kind === 'per-call-approval';
        }
        return true;
      });
      return json(response, 200, { items });
    }

    if (request.url === '/v1/admin/ui/wrapper/set-mode' && request.method === 'POST') {
      const wrapper = String(body.wrapper ?? '');
      const mode = body.mode as WrapperMode;
      const reason = String(body.reason ?? '') || null;
      const entry = state.wrappers.find((item) => item.wrapper === wrapper);
      if (entry) {
        entry.mode = mode;
        entry.reason = reason;
      }
      pushAudit(state, { action: 'wrapper_mode_updated', wrapper, resource: null, secret_id: null });
      return json(response, 200, { wrapper, mode, reason });
    }

    if (request.url === '/v1/admin/ui/matrix/query' && request.method === 'POST') {
      const resource = String(body.resource ?? 'gh/main');
      const hostFilter = String(body.host ?? '');
      const userFilter = String(body.user ?? '');
      const wrapperFilter = String(body.wrapper ?? '');
      const rows = [
        { host: 'deepthought', user: 'robert', wrapper: 'gh-authorized' },
        { host: 'smallthought', user: 'robert', wrapper: 'jira-readonly' },
      ].filter((row) => (!hostFilter || row.host.includes(hostFilter)) && (!userFilter || row.user.includes(userFilter)) && (!wrapperFilter || row.wrapper.includes(wrapperFilter)))
        .map((row) => ({
          ...row,
          cells: state.matrix[`${row.wrapper}|${resource}`] ?? {
            'env.read': false,
            'files.read': false,
            'files.write': false,
            'libsecret.read': false,
            'libsecret.write': false,
          },
        }));
      return json(response, 200, {
        operations: ['env.read', 'files.read', 'files.write', 'libsecret.read', 'libsecret.write'],
        rows,
      });
    }

    if (request.url === '/v1/admin/ui/matrix/set-cell' && request.method === 'POST') {
      const wrapper = String(body.wrapper ?? '');
      const resource = String(body.resource ?? '');
      const operation = String(body.operation ?? '');
      const enabled = Boolean(body.enabled);
      state.matrix[`${wrapper}|${resource}`] ??= {};
      state.matrix[`${wrapper}|${resource}`][operation] = enabled;
      pushAudit(state, {
        action: enabled ? 'matrix_cell_enabled' : 'matrix_cell_disabled',
        wrapper,
        resource,
        operation,
        secret_id: null,
      });
      return json(response, 200, { enabled });
    }

    if (request.url === '/v1/admin/ui/audit/query' && request.method === 'POST') {
      const operator = String(body.operator ?? '');
      const wrapper = String(body.wrapper ?? '');
      const resource = String(body.resource ?? '');
      const action = String(body.action ?? '');
      const fromEpoch = typeof body.from_epoch === 'number' ? body.from_epoch : Number(body.from_epoch ?? 0) || 0;
      const toEpoch = typeof body.to_epoch === 'number' ? body.to_epoch : Number(body.to_epoch ?? 0) || Number.MAX_SAFE_INTEGER;
      const items = state.audit
        .filter((event) => (!operator || event.operator === operator) && (!wrapper || event.wrapper === wrapper) && (!resource || event.resource === resource) && (!action || event.action === action) && event.ts_epoch >= fromEpoch && event.ts_epoch <= toEpoch)
        .sort((a, b) => b.ts_epoch - a.ts_epoch);
      return json(response, 200, { items, total: items.length });
    }

    return notFound(response);
  });

  await new Promise<void>((resolve) => server.listen(port, '127.0.0.1', () => resolve()));
  return {
    close: () => new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))),
  };
}
