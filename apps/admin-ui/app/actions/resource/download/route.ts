import { getConfiguredBackendClient } from '../../../../lib/server/backend-client.ts';
import { requireActionSession } from '../../../../lib/server/resource-action.ts';

export async function POST(request: Request) {
  const formData = await request.formData();
  const auth = await requireActionSession(request, String(formData.get('csrf_token') ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const client = await getConfiguredBackendClient();
  const response = await client.downloadResource(
    String(formData.get('store') ?? ''),
    String(formData.get('secret_id') ?? ''),
  );

  return new Response(response.body, {
    status: response.status,
    headers: {
      'Content-Type': response.headers.get('content-type') ?? 'application/octet-stream',
      'Content-Disposition': response.headers.get('content-disposition') ?? 'attachment',
      'Cache-Control': 'no-store',
    },
  });
}
