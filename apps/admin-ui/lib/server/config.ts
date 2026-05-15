import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';

import { cache } from 'react';
import YAML from 'yaml';

type RawConfig = {
  session_key_file?: string;
  operators?: Record<string, { password_hash_file?: string }>;
  backends?: Record<string, { admin_url?: string; token_file?: string }>;
};

type LocalOperatorState = {
  version?: number;
  operators?: Record<string, { password_hash?: string }>;
};

export type LoadedOperator = {
  name: string;
  passwordHash: string;
  source: 'config' | 'state';
};

export type LoadedBackend = {
  name: string;
  adminUrl: string;
  token: string;
};

type LoadedStaticConfig = {
  sourcePath: string;
  stateDir: string;
  operatorStatePath: string;
  sessionKey: Buffer;
  configuredOperators: Record<string, LoadedOperator>;
  backends: Record<string, LoadedBackend>;
};

export type LoadedConfig = {
  sourcePath: string;
  stateDir: string;
  operatorStatePath: string;
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

function defaultStateDir() {
  return path.join(/* turbopackIgnore: true */ process.cwd(), 'dev-fixtures', 'state');
}

function resolveStateDir() {
  return process.env.UNDERWRAP_ADMIN_UI_STATE_DIR ?? defaultStateDir();
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

function normalizeOperatorName(name: string) {
  return name.trim();
}

async function loadStaticConfigUnchecked(configPath: string, stateDir: string): Promise<LoadedStaticConfig> {
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
  const backendEntries = Object.entries(parsed.backends ?? {});
  if (backendEntries.length === 0) {
    throw new Error(`admin UI config at ${configPath} must define at least one backend target`);
  }

  const configuredOperators = Object.fromEntries(
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
        const normalizedName = normalizeOperatorName(name);
        return [
          normalizedName,
          { name: normalizedName, passwordHash, source: 'config' } satisfies LoadedOperator,
        ] as const;
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
    stateDir,
    operatorStatePath: path.join(stateDir, 'operators.json'),
    sessionKey: await readSessionKey(resolveSibling(configPath, sessionKeyFile)),
    configuredOperators,
    backends,
  } satisfies LoadedStaticConfig;
}

const loadStaticRuntimeConfig = cache(loadStaticConfigUnchecked);

async function loadStateOperators(operatorStatePath: string) {
  const raw = await readFile(operatorStatePath, 'utf8').catch((error: NodeJS.ErrnoException | unknown) => {
    if (typeof error === 'object' && error !== null && 'code' in error && error.code === 'ENOENT') {
      return null;
    }
    throw new Error(`failed to read operator state file at ${operatorStatePath}: ${String(error)}`);
  });

  if (raw === null) {
    return {} as Record<string, LoadedOperator>;
  }

  const parsed = JSON.parse(raw) as LocalOperatorState;
  const entries = Object.entries(parsed.operators ?? {});
  const operators = Object.fromEntries(
    entries.map(([name, operator]) => {
      const normalizedName = normalizeOperatorName(name);
      const passwordHash = operator.password_hash?.trim();
      if (!normalizedName || !passwordHash?.startsWith('$argon2')) {
        throw new Error(`operator state file at ${operatorStatePath} contains an invalid operator record`);
      }
      return [
        normalizedName,
        { name: normalizedName, passwordHash, source: 'state' } satisfies LoadedOperator,
      ] as const;
    }),
  );

  return operators;
}

async function writeOperatorState(operatorStatePath: string, state: LocalOperatorState) {
  await mkdir(path.dirname(operatorStatePath), { recursive: true });
  const tempPath = `${operatorStatePath}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(tempPath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600 });
  await rename(tempPath, operatorStatePath);
}

export async function loadRuntimeConfig(): Promise<LoadedConfig> {
  const configPath = resolveConfigPath();
  const stateDir = resolveStateDir();
  const base = await loadStaticRuntimeConfig(configPath, stateDir);
  const stateOperators = await loadStateOperators(base.operatorStatePath);
  return {
    sourcePath: base.sourcePath,
    stateDir: base.stateDir,
    operatorStatePath: base.operatorStatePath,
    sessionKey: base.sessionKey,
    operators: {
      ...stateOperators,
      ...base.configuredOperators,
    },
    backends: base.backends,
  };
}

export async function bootstrapRequired() {
  const config = await loadRuntimeConfig();
  return Object.keys(config.operators).length === 0;
}

export async function storeBootstrappedOperator(name: string, passwordHash: string) {
  const config = await loadRuntimeConfig();
  if (Object.keys(config.operators).length > 0) {
    throw new Error('admin UI bootstrap is already complete');
  }

  const normalizedName = normalizeOperatorName(name);
  if (!normalizedName) {
    throw new Error('operator username is required');
  }
  if (!passwordHash.startsWith('$argon2')) {
    throw new Error('bootstrap password hash is invalid');
  }

  await writeOperatorState(config.operatorStatePath, {
    version: 1,
    operators: {
      [normalizedName]: {
        password_hash: passwordHash,
      },
    },
  });
}

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
