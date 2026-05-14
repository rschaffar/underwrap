import { BackendApiError, getBackendClient } from '../../../../lib/server/backend-client.ts';
import { requireActionSession } from '../../../../lib/server/resource-action.ts';
import { redirectToWrappers } from '../../../../lib/server/wrappers-action.ts';

function flashForError(error: unknown) {
  if (error instanceof BackendApiError) {
    return 'wrapper-mode-failed';
  }
  return 'wrapper-mode-failed';
}

export async function POST(request: Request) {
  const formData = await request.formData();
  const auth = await requireActionSession(request, String(formData.get('csrf_token') ?? ''));
  if ('error' in auth) {
    return auth.error;
  }

  const target = String(formData.get('target') ?? '');
  const store = String(formData.get('store') ?? '');
  const wrapper = String(formData.get('wrapper') ?? '');
  const modeKind = String(formData.get('mode_kind') ?? 'locked');
  const reason = String(formData.get('reason') ?? '');
  const query = {
    q: String(formData.get('q') ?? ''),
    mode: String(formData.get('mode') ?? ''),
    resource: String(formData.get('resource') ?? ''),
    host: String(formData.get('host') ?? ''),
    user: String(formData.get('user') ?? ''),
    matrix_wrapper: String(formData.get('matrix_wrapper') ?? ''),
  };

  const client = await getBackendClient(target);
  if (!client) {
    return new Response('unknown target', { status: 404 });
  }

  try {
    let mode: Record<string, unknown>;
    switch (modeKind) {
      case 'locked':
        mode = { kind: 'locked' };
        break;
      case 'unlocked':
        mode = { kind: 'unlocked' };
        break;
      case 'unlocked_until': {
        const untilEpoch = Number(formData.get('until_epoch') ?? '0');
        if (!Number.isFinite(untilEpoch) || untilEpoch <= 0) {
          return redirectToWrappers(request, target, store, { ...query, flash: 'wrapper-mode-invalid-until' });
        }
        mode = { kind: 'unlocked', until_epoch: Math.floor(untilEpoch) };
        break;
      }
      case 'per_call_approval': {
        const approvalKey = String(formData.get('approval_key') ?? '').trim();
        if (!approvalKey) {
          return redirectToWrappers(request, target, store, { ...query, flash: 'wrapper-mode-missing-key' });
        }
        mode = { kind: 'per-call-approval', approval_key: approvalKey };
        break;
      }
      default:
        return redirectToWrappers(request, target, store, { ...query, flash: 'wrapper-mode-invalid' });
    }

    await client.setWrapperMode(store, wrapper, mode, reason);
    return redirectToWrappers(request, target, store, { ...query, flash: 'wrapper-mode-saved' });
  } catch (error) {
    return redirectToWrappers(request, target, store, { ...query, flash: flashForError(error) });
  }
}
