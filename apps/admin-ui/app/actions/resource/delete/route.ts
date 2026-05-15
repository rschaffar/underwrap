import { BackendApiError, getConfiguredBackendClient } from '../../../../lib/server/backend-client.ts';
import { redirectToResourceDetail, requireActionSession } from '../../../../lib/server/resource-action.ts';
import { parseSecretId } from '../../../../lib/server/resource-id.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError && error.status === 403) {
    return 'mutation-denied';
  }
  return 'delete-failed';
}

export async function POST(request: Request) {
  const formData = await request.formData();
  const auth = await requireActionSession(request, String(formData.get('csrf_token') ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const store = String(formData.get('store') ?? '');
  const wrapper = String(formData.get('wrapper') ?? '');
  const secretId = String(formData.get('secret_id') ?? '');
  const resource = String(formData.get('resource') ?? '');

  const client = await getConfiguredBackendClient();

  try {
    await client.deleteResourceEntry(store, wrapper, secretId);
    return redirectToResourceDetail(request, store, resource || parseSecretId(secretId).resource, 'secret-deleted');
  } catch (error) {
    return redirectToResourceDetail(request, store, resource, flashForError(error));
  }
}
