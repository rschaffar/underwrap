import { bootstrapRequired, loadRuntimeConfig, storeBootstrappedOperator } from './config.ts';
import { createPasswordHash, verifyPassword } from './passwords.ts';
import { createSession } from './session-core.ts';

function normalizeUsername(username: string) {
  return username.trim();
}

export async function authenticateOperator(username: string, password: string) {
  const normalizedUsername = normalizeUsername(username);
  if (!normalizedUsername) {
    return null;
  }

  const config = await loadRuntimeConfig();
  const operator = config.operators[normalizedUsername];
  if (!operator) {
    return null;
  }

  const passwordMatches = await verifyPassword(password, operator.passwordHash);
  if (!passwordMatches) {
    return null;
  }

  return createSession(operator.name);
}

export async function adminBootstrapRequired() {
  return bootstrapRequired();
}

export async function bootstrapFirstOperator(username: string, password: string) {
  const normalizedUsername = normalizeUsername(username);
  if (!normalizedUsername) {
    throw new Error('operator username is required');
  }
  if (!password) {
    throw new Error('password is required');
  }

  if (!(await bootstrapRequired())) {
    throw new Error('admin UI bootstrap is already complete');
  }

  const passwordHash = await createPasswordHash(password);
  await storeBootstrappedOperator(normalizedUsername, passwordHash);
  return createSession(normalizedUsername);
}
