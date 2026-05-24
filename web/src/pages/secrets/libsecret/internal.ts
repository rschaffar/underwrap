// Shared helpers for the libsecret tree UI. Kept private to the
// `libsecret/` module — the only public surface is `LibsecretTree`,
// re-exported via `./index.ts`.

// Operator-supplied ids are validated by the backend with
// `[A-Za-z0-9._-]+`, max 128, no leading `_`. Mirror the rule in the
// client for inline form feedback; failure messages from the backend
// remain authoritative.
export const ID_PATTERN = /^(?!_)[A-Za-z0-9._-]+$/;

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
