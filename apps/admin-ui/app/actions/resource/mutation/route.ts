import { BackendApiError, getConfiguredBackendClient } from '../../../../lib/server/backend-client.ts';
import { redirectToResourceDetail, requireActionSession } from '../../../../lib/server/resource-action.ts';
import { buildSecretId } from '../../../../lib/server/resource-id.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError && error.status === 403) {
    return 'mutation-denied';
  }
  return 'mutation-failed';
}

export async function POST(request: Request) {
  const formData = await request.formData();
  const auth = await requireActionSession(request, String(formData.get('csrf_token') ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const store = String(formData.get('store') ?? '');
  const resource = String(formData.get('resource') ?? '');
  const wrapper = String(formData.get('wrapper') ?? '');
  const mode = String(formData.get('mode') ?? 'temporary');

  const client = await getConfiguredBackendClient();

  try {
    const secretId = String(formData.get('secret_id') ?? '') || buildSecretId(resource, String(formData.get('slot') ?? '') as 'env' | 'files' | 'libsecret', String(formData.get('entry') ?? ''));
    await client.setMutation(store, wrapper, secretId, mode === 'disabled' ? 'disabled' : 'temporary');
    return redirectToResourceDetail(request, store, resource, mode === 'disabled' ? 'mutation-disabled' : 'mutation-enabled');
  } catch (error) {
    return redirectToResourceDetail(request, store, resource, flashForError(error));
  }
}
