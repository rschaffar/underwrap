import { BackendApiError, getConfiguredBackendClient } from '../../../../lib/server/backend-client.ts';
import { redirectToResourceDetail, requireActionSession } from '../../../../lib/server/resource-action.ts';
import { buildSecretId } from '../../../../lib/server/resource-id.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError && error.status === 403) {
    return 'mutation-denied';
  }
  if (error instanceof BackendApiError && error.status === 413) {
    return 'payload-too-large';
  }
  return 'write-failed';
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
  const slot = String(formData.get('slot') ?? '') as 'env' | 'libsecret';
  const entry = String(formData.get('entry') ?? '');
  const value = String(formData.get('value') ?? '');

  const client = await getConfiguredBackendClient();

  try {
    const secretId = buildSecretId(resource, slot, entry);
    await client.writeTextResource(store, wrapper, secretId, value);
    return redirectToResourceDetail(request, store, resource, 'secret-saved');
  } catch (error) {
    return redirectToResourceDetail(request, store, resource, flashForError(error));
  }
}
