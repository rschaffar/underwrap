import { readFile } from 'node:fs/promises';
import path from 'node:path';

import { cache } from 'react';
import YAML from 'yaml';

type RawConfig = {
  session_key_file?: string;
  operators?: Record<string, { password_hash_file?: string }>;
  backends?: Record<string, { admin_url?: string; token_file?: string }>;
};

export type LoadedOperator = {
  name: string;
  passwordHash: string;
};

export type LoadedBackend = {
  name: string;
  adminUrl: string;
  token: string;
};

export type LoadedConfig = {
  sourcePath: string;
  sessionKey: Buffer;
  operators: Record<string, LoadedOperator>;
  backends: Record<string, LoadedBackend>;
};

function defaultConfigPath() {
  return path.join(/* turbopackIgnore: true */ process.cwd(), 'dev-fixtures', 'admin-ui.yaml');
}

function resolveConfigPath() {
  return process.env.UNDERWRAP_ADMIN_UI_CONFIG ?? defaultConfigPath();
}

function resolveSibling(baseFile: string, maybeRelative: string) {
  return path.isAbsolute(maybeRelative)
    ? maybeRelative
    : path.resolve(/* turbopackIgnore: true */ path.dirname(baseFile), maybeRelative);
}

async function readTrimmedText(filePath: string, label: string) {
  const raw = await readFile(filePath, 'utf8').catch((error: unknown) => {
    throw new Error(`failed to read ${label} file at ${filePath}: ${String(error)}`);
  });
  const value = raw.trim();
  if (!value) {
    throw new Error(`${label} file at ${filePath} is empty`);
  }
  return value;
}

async function readSessionKey(filePath: string) {
  const raw = await readFile(filePath).catch((error: unknown) => {
    throw new Error(`failed to read session key file at ${filePath}: ${String(error)}`);
  });
  if (raw.length !== 32) {
    throw new Error(`session key file at ${filePath} must contain exactly 32 bytes (got ${raw.length})`);
  }
  return raw;
}

async function loadConfigUnchecked() {
  const configPath = resolveConfigPath();
  const configRaw = await readFile(configPath, 'utf8').catch((error: unknown) => {
    throw new Error(`failed to read admin UI config at ${configPath}: ${String(error)}`);
  });

  const parsed = YAML.parse(configRaw) as RawConfig;
  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`admin UI config at ${configPath} did not parse to an object`);
  }

  const sessionKeyFile = parsed.session_key_file;
  if (!sessionKeyFile) {
    throw new Error(`admin UI config at ${configPath} is missing session_key_file`);
  }

  const operatorEntries = Object.entries(parsed.operators ?? {});
  if (operatorEntries.length === 0) {
    throw new Error(`admin UI config at ${configPath} must define at least one operator`);
  }

  const backendEntries = Object.entries(parsed.backends ?? {});
  if (backendEntries.length === 0) {
    throw new Error(`admin UI config at ${configPath} must define at least one backend target`);
  }

  const operators = Object.fromEntries(
    await Promise.all(
      operatorEntries.map(async ([name, operator]) => {
        if (!operator.password_hash_file) {
          throw new Error(`operator ${name} is missing password_hash_file`);
        }
        const passwordHash = await readTrimmedText(
          resolveSibling(configPath, operator.password_hash_file),
          `password hash for operator ${name}`,
        );
        if (!passwordHash.startsWith('$argon2')) {
          throw new Error(`password hash for operator ${name} is not a PHC-encoded argon2 hash`);
        }
        return [name, { name, passwordHash } satisfies LoadedOperator] as const;
      }),
    ),
  );

  const backends = Object.fromEntries(
    await Promise.all(
      backendEntries.map(async ([name, backend]) => {
        if (!backend.admin_url) {
          throw new Error(`backend ${name} is missing admin_url`);
        }
        if (!backend.token_file) {
          throw new Error(`backend ${name} is missing token_file`);
        }
        let adminUrl: string;
        try {
          adminUrl = new URL(backend.admin_url).toString().replace(/\/$/, '');
        } catch {
          throw new Error(`backend ${name} has an invalid admin_url`);
        }
        const token = await readTrimmedText(
          resolveSibling(configPath, backend.token_file),
          `admin token for backend ${name}`,
        );
        return [name, { name, adminUrl, token } satisfies LoadedBackend] as const;
      }),
    ),
  );

  return {
    sourcePath: configPath,
    sessionKey: await readSessionKey(resolveSibling(configPath, sessionKeyFile)),
    operators,
    backends,
  } satisfies LoadedConfig;
}

export const loadRuntimeConfig = cache(loadConfigUnchecked);

export async function listConfiguredTargets() {
  const config = await loadRuntimeConfig();
  return Object.values(config.backends).map((backend) => ({
    name: backend.name,
    adminUrl: backend.adminUrl,
  }));
}

export async function getConfiguredBackend(name: string) {
  const config = await loadRuntimeConfig();
  return config.backends[name];
}
