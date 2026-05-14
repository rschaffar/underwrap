import { BackendApiError, getBackendClient } from '../../../../lib/server/backend-client.ts';
import { requireActionSession } from '../../../../lib/server/resource-action.ts';
import { redirectToWrappers } from '../../../../lib/server/wrappers-action.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError) {
    return 'matrix-toggle-failed';
  }
  return 'matrix-toggle-failed';
}

export async function POST(request: Request) {
  const formData = await request.formData();
  const auth = await requireActionSession(request, String(formData.get('csrf_token') ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const target = String(formData.get('target') ?? '');
  const store = String(formData.get('store') ?? '');
  const resource = String(formData.get('resource') ?? '');
  const wrapper = String(formData.get('wrapper') ?? '');
  const operation = String(formData.get('operation') ?? '');
  const enabled = String(formData.get('enabled') ?? 'false') === 'true';
  const query = {
    q: String(formData.get('q') ?? ''),
    mode: String(formData.get('mode') ?? ''),
    resource,
    host: String(formData.get('host') ?? ''),
    user: String(formData.get('user') ?? ''),
    matrix_wrapper: String(formData.get('matrix_wrapper') ?? ''),
  };

  const client = await getBackendClient(target);
  if (!client) {
    return new Response('unknown target', { status: 404 });
  }

  try {
    await client.setMatrixCell(store, wrapper, resource, operation, enabled);
    return redirectToWrappers(request, target, store, { ...query, flash: enabled ? 'matrix-cell-enabled' : 'matrix-cell-disabled' });
  } catch (error) {
    return redirectToWrappers(request, target, store, { ...query, flash: flashForError(error) });
  }
}
