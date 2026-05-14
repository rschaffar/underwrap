export const ADMIN_UI_MAX_FILE_BYTES = 1024 * 1024;

export type ResourceSlot = 'env' | 'files' | 'libsecret';

function isReservedEnvName(name: string) {
  return (
    name.startsWith('LD_') ||
    [
      'LD_PRELOAD',
      'LD_LIBRARY_PATH',
      'LD_AUDIT',
      'PATH',
      'HOME',
      'USER',
      'LOGNAME',
      'SHELL',
      'DBUS_SESSION_BUS_ADDRESS',
      'SSH_AUTH_SOCK',
    ].includes(name) ||
    name.startsWith('UNDERWRAP_')
  );
}

export function validateEnvName(name: string) {
  if (!name) {
    throw new Error('environment variable name is empty');
  }
  if (!/^[_A-Za-z][_A-Za-z0-9]*$/.test(name)) {
    throw new Error("environment variable name must start with '_' or an ASCII letter and contain only ASCII letters, digits, or '_'");
  }
  if (isReservedEnvName(name)) {
    throw new Error('environment variable name is reserved by underwrap hardening');
  }
}

export function validateRelativeItemPath(path: string) {
  if (!path || path.startsWith('/') || path.includes('\0')) {
    throw new Error('path must be a non-empty relative path');
  }
  for (const segment of path.split('/')) {
    if (!segment || segment === '.' || segment === '..') {
      throw new Error('path must be clean and must not contain empty segments');
    }
  }
}

export function buildSecretId(resource: string, slot: ResourceSlot, entry: string) {
  const trimmedResource = resource.trim();
  const trimmedEntry = entry.trim();
  if (!trimmedResource) {
    throw new Error('resource is required');
  }
  if (slot === 'env') {
    validateEnvName(trimmedEntry);
  } else {
    validateRelativeItemPath(trimmedEntry);
  }
  return `${trimmedResource}/${slot}/${trimmedEntry}`;
}

export function parseSecretId(secretId: string) {
  for (const slot of ['env', 'files', 'libsecret'] as const) {
    const marker = `/${slot}/`;
    const index = secretId.indexOf(marker);
    if (index === -1) {
      continue;
    }
    return {
      resource: secretId.slice(0, index),
      slot,
      entry: secretId.slice(index + marker.length),
    };
  }
  throw new Error('secret_id must identify an env, files, or libsecret entry');
}
