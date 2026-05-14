import { getBackendClient } from '../../../../lib/server/backend-client.ts';
import { requireActionSession } from '../../../../lib/server/resource-action.ts';

export async function POST(request: Request) {
  const body = (await request.json()) as {
    target?: string;
    store?: string;
    secret_id?: string;
    csrf_token?: string;
  };
  const auth = await requireActionSession(request, String(body.csrf_token ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const client = await getBackendClient(String(body.target ?? ''));
  if (!client) {
    return new Response('unknown target', { status: 404 });
  }

  const response = await client.revealResource(String(body.store ?? ''), String(body.secret_id ?? ''));
  return Response.json(response, {
    headers: {
      'Cache-Control': 'no-store',
    },
  });
}
