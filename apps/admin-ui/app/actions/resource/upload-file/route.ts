import { BackendApiError, getBackendClient } from '../../../../lib/server/backend-client.ts';
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

  const target = String(formData.get('target') ?? '');
  const store = String(formData.get('store') ?? '');
  const resource = String(formData.get('resource') ?? '');
  const wrapper = String(formData.get('wrapper') ?? '');
  const path = String(formData.get('entry') ?? '');
  const file = formData.get('file');

  const client = await getBackendClient(target);
  if (!client) {
    return new Response('unknown target', { status: 404 });
  }
  if (!(file instanceof File)) {
    return redirectToResourceDetail(request, target, store, resource, 'upload-failed');
  }

  try {
    const bytes = Buffer.from(await file.arrayBuffer());
    if (bytes.length > ADMIN_UI_MAX_FILE_BYTES) {
      return redirectToResourceDetail(request, target, store, resource, 'payload-too-large');
    }
    const secretId = buildSecretId(resource, 'files', path);
    await client.uploadFileResource(store, wrapper, secretId, bytes.toString('base64'));
    return redirectToResourceDetail(request, target, store, resource, 'secret-saved');
  } catch (error) {
    return redirectToResourceDetail(request, target, store, resource, flashForError(error));
  }
}
