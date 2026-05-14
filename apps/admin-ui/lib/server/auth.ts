import { loadRuntimeConfig } from './config.ts';
import { verifyPassword } from './passwords.ts';
import { createSession } from './session-core.ts';

export async function authenticateOperator(username: string, password: string) {
  const normalizedUsername = username.trim();
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
