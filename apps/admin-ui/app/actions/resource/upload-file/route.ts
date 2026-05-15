import { BackendApiError, getConfiguredBackendClient } from '../../../../lib/server/backend-client.ts';
import { redirectToResourceDetail, requireActionSession } from '../../../../lib/server/resource-action.ts';
import { ADMIN_UI_MAX_FILE_BYTES, buildSecretId } from '../../../../lib/server/resource-id.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError && error.status === 403) {
    return 'mutation-denied';
  }
  if (error instanceof BackendApiError && error.status === 413) {
    return 'payload-too-large';
  }
  return 'upload-failed';
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
  const path = String(formData.get('entry') ?? '');
  const file = formData.get('file');

  const client = await getConfiguredBackendClient();
  if (!(file instanceof File)) {
    return redirectToResourceDetail(request, store, resource, 'upload-failed');
  }

  try {
    const bytes = Buffer.from(await file.arrayBuffer());
    if (bytes.length > ADMIN_UI_MAX_FILE_BYTES) {
      return redirectToResourceDetail(request, store, resource, 'payload-too-large');
    }
    const secretId = buildSecretId(resource, 'files', path);
    await client.uploadFileResource(store, wrapper, secretId, bytes.toString('base64'));
    return redirectToResourceDetail(request, store, resource, 'secret-saved');
  } catch (error) {
    return redirectToResourceDetail(request, store, resource, flashForError(error));
  }
}
