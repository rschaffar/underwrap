import { randomBytes, timingSafeEqual } from 'node:crypto';

const EMPTY = new Uint8Array();
const PHC_PATTERN = /^\$(argon2id|argon2i|argon2d)\$v=(\d+)\$m=(\d+),t=(\d+),p=(\d+)\$([^$]+)\$([^$]+)$/;
const DEFAULT_MEMORY_KIB = 19_456;
const DEFAULT_ITERATIONS = 2;
const DEFAULT_PARALLELISM = 1;
const DEFAULT_SALT_BYTES = 16;

function decodeBase64(value: string) {
  const normalized = value + '='.repeat((4 - (value.length % 4 || 4)) % 4);
  return Buffer.from(normalized, 'base64');
}

function hashTypeFor(identifier: string, hashType: { Argon2d: number; Argon2i: number; Argon2id: number }) {
  switch (identifier) {
    case 'argon2d':
      return hashType.Argon2d;
    case 'argon2i':
      return hashType.Argon2i;
    case 'argon2id':
      return hashType.Argon2id;
    default:
      return null;
  }
}

function encodeBase64Unpadded(input: Uint8Array | Buffer) {
  return Buffer.from(input).toString('base64').replace(/=+$/g, '');
}

export async function createPasswordHash(password: string) {
  const { HashType, argon2 } = await import('@whi/argon2-wasm');
  const salt = randomBytes(DEFAULT_SALT_BYTES);
  const hash = Buffer.from(
    argon2(
      Buffer.from(password, 'utf8'),
      salt,
      EMPTY,
      EMPTY,
      DEFAULT_ITERATIONS,
      DEFAULT_PARALLELISM,
      DEFAULT_MEMORY_KIB,
      HashType.Argon2id,
    ),
  );
  const encoded = `$argon2id$v=19$m=${DEFAULT_MEMORY_KIB},t=${DEFAULT_ITERATIONS},p=${DEFAULT_PARALLELISM}$${encodeBase64Unpadded(salt)}$${encodeBase64Unpadded(hash)}`;

  if (!PHC_PATTERN.test(encoded)) {
    throw new Error('generated password hash is not a valid Argon2 PHC string');
  }

  return encoded;
}

export async function verifyPassword(password: string, phcHash: string) {
  const match = PHC_PATTERN.exec(phcHash);
  if (!match) {
    return false;
  }

  const [, identifier, versionText, memoryText, iterationsText, parallelismText, saltB64, hashB64] = match;
  if (versionText !== '19') {
    return false;
  }

  const { HashType, argon2 } = await import('@whi/argon2-wasm');
  const hashType = hashTypeFor(identifier, HashType);
  if (hashType === null) {
    return false;
  }

  const memory = Number.parseInt(memoryText, 10);
  const iterations = Number.parseInt(iterationsText, 10);
  const parallelism = Number.parseInt(parallelismText, 10);
  const salt = decodeBase64(saltB64);
  const expectedHash = decodeBase64(hashB64);

  if (
    !Number.isFinite(memory) ||
    !Number.isFinite(iterations) ||
    !Number.isFinite(parallelism) ||
    memory <= 0 ||
    iterations <= 0 ||
    parallelism <= 0 ||
    salt.length === 0 ||
    expectedHash.length === 0
  ) {
    return false;
  }

  const derivedHash = Buffer.from(
    argon2(
      Buffer.from(password, 'utf8'),
      salt,
      EMPTY,
      EMPTY,
      iterations,
      parallelism,
      memory,
      hashType,
    ),
  );

  return derivedHash.length === expectedHash.length && timingSafeEqual(derivedHash, expectedHash);
}
