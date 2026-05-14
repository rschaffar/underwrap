export type NavSection = 'overview' | 'resources' | 'wrappers' | 'audit';

export function buildResourcePath(resource: string[]) {
  return resource.join('/');
}
