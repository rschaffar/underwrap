const BINARY_STRING_CHUNK_SIZE = 32 * 1024;

export function encodeUtf8Base64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  const chunks: string[] = [];

  for (let offset = 0; offset < bytes.length; offset += BINARY_STRING_CHUNK_SIZE) {
    chunks.push(
      String.fromCharCode(
        ...bytes.subarray(offset, offset + BINARY_STRING_CHUNK_SIZE),
      ),
    );
  }

  return btoa(chunks.join(""));
}

export type DecodeBase64Utf8Result =
  | { ok: true; text: string }
  | { ok: false; reason: "invalid-base64" | "invalid-utf8" };

export function decodeBase64Utf8(value: string): DecodeBase64Utf8Result {
  let binary: string;
  try {
    binary = atob(value);
  } catch {
    return { ok: false, reason: "invalid-base64" };
  }

  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  try {
    return {
      ok: true,
      // A leading BOM is value content, not a transport signature to consume.
      text: new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes),
    };
  } catch {
    return { ok: false, reason: "invalid-utf8" };
  }
}
