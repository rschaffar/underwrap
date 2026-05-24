/**
 * Epoch helpers shared across admin pages.
 *
 * u64 fields from the backend arrive as `bigint` (ts-rs default) but the
 * Date range we'd ever display comfortably fits in a JS `number`, so the
 * helpers fold the bigint coercion in once and operate on plain numbers
 * thereafter.
 */

/**
 * Coerce a backend epoch field to a `number`, returning `null` for
 * absent / zero / NaN values so callers can collapse the empty case.
 */
export function normalizeEpoch(
  value: bigint | number | null | undefined,
): number | null {
  if (value === null || value === undefined) return null;
  const n = typeof value === "bigint" ? Number(value) : value;
  if (!n || Number.isNaN(n)) return null;
  return n;
}

/** Render an epoch as a relative label ("3 min ago"). */
export function formatEpochRelative(
  value: bigint | number | null | undefined,
  now: number = Date.now(),
): string {
  const n = normalizeEpoch(value);
  if (n === null) return "never";
  const diffSecs = Math.round(now / 1000 - n);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  const abs = Math.abs(diffSecs);
  if (abs < 60) return rtf.format(-diffSecs, "second");
  if (abs < 3600) return rtf.format(-Math.round(diffSecs / 60), "minute");
  if (abs < 86400) return rtf.format(-Math.round(diffSecs / 3600), "hour");
  return rtf.format(-Math.round(diffSecs / 86400), "day");
}

/** Render an epoch as a full local timestamp (date + time). */
export function formatEpochAbsolute(
  value: bigint | number | null | undefined,
): string {
  const n = normalizeEpoch(value);
  if (n === null) return "never";
  return new Date(n * 1000).toLocaleString();
}

/** Convert a backend epoch (seconds) to a `Date`. */
export function epochToDate(epoch: number): Date {
  return new Date(epoch * 1000);
}

/** Render an epoch as a date-only label. */
export function formatEpochDate(
  value: bigint | number | null | undefined,
): string {
  const n = normalizeEpoch(value);
  if (n === null) return "never";
  return new Date(n * 1000).toLocaleDateString();
}

/**
 * Format a `Date` as the `YYYY-MM-DDTHH:MM` string that
 * `<input type="datetime-local">` expects (browser-local timezone, no
 * timezone suffix). The tzoffset arithmetic shifts a UTC Date back into
 * local space before slicing the ISO string.
 */
export function toDatetimeLocal(date: Date): string {
  const offsetMs = date.getTimezoneOffset() * 60_000;
  const local = new Date(date.getTime() - offsetMs);
  return local.toISOString().slice(0, 16);
}
