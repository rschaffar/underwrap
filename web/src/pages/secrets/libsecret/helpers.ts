import { z } from "zod";

// Operator-supplied ids are validated by the backend with
// `[A-Za-z0-9._-]+`, max 128, no leading `_`. Mirror the rule in the
// client for inline form feedback; failure messages from the backend
// remain authoritative.
const ID_PATTERN = /^(?!_)[A-Za-z0-9._-]+$/;
const ID_MESSAGE = "Id must match [A-Za-z0-9._-]+ and not start with '_'.";

/**
 * Zod refinement for libsecret-style ids and alias names. Non-empty,
 * matches the operator-id pattern.
 */
export function libsecretIdField() {
  return z
    .string()
    .min(1, "Id is required.")
    .refine((s) => ID_PATTERN.test(s), ID_MESSAGE);
}

/**
 * Variant used by create-collection / create-item where a blank value
 * means "auto-generate a stable id server-side". Validates the
 * pattern only when the operator typed something.
 */
export function optionalLibsecretIdField() {
  return z
    .string()
    .optional()
    .refine((s) => !s || ID_PATTERN.test(s), ID_MESSAGE);
}

export function libsecretTreeKey(storeId: string, resourceId: string) {
  return ["admin", "libsecret", storeId, resourceId] as const;
}

export function parseAttributes(input: string): Record<string, string> {
  // `key=value` per line; blank lines and lines without `=` ignored.
  // Keys / values trimmed. Duplicate keys: last wins.
  const out: Record<string, string> = {};
  for (const line of input.split("\n")) {
    const idx = line.indexOf("=");
    if (idx < 0) continue;
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    if (!key) continue;
    out[key] = value;
  }
  return out;
}
