import { z } from "zod";

// Mirror strict publication identifiers for new entries only. Stored legacy
// identifiers must still be displayable and deletable. Backend checks own admission.
// Reject punctuation, non-ASCII characters and all whitespace without normalization.
const INVALID_ID_CHARACTER = /[^A-Za-z0-9_]/;
const ID_MESSAGE = "Id must contain only ASCII letters, digits or underscores.";
const ID_MAX = 128;
const ID_MAX_MESSAGE = "Id must be at most 128 characters.";

/**
 * Collection ids and alias names, including leading digits/underscores.
 */
export function libsecretIdField() {
  return z
    .string()
    .min(1, "Id is required.")
    .max(ID_MAX, ID_MAX_MESSAGE)
    .refine((s) => !INVALID_ID_CHARACTER.test(s), ID_MESSAGE);
}

/**
 * Variant used by create-collection where a blank value
 * means "auto-generate a stable id server-side". Validates the
 * pattern only when the operator typed something.
 */
export function optionalLibsecretIdField() {
  return z
    .string()
    .max(ID_MAX, ID_MAX_MESSAGE)
    .optional()
    .refine((s) => !s || !INVALID_ID_CHARACTER.test(s), ID_MESSAGE);
}

export function optionalLibsecretItemIdField() {
  return z.string().optional().refine(
    (s) => !s || (s.length === 32 && !/[^0-9a-f]/.test(s)),
    "Item id must contain exactly 32 lowercase hexadecimal characters.",
  );
}

export function libsecretTreeKey(storeId: string, resourceId: string) {
  return ["admin", "libsecret", storeId, resourceId] as const;
}

export type AttributeParseErrorReason =
  | "malformed-line"
  | "empty-key"
  | "duplicate-key";

export class AttributeParseError extends Error {
  readonly reason: AttributeParseErrorReason;
  readonly lineNumber: number;
  readonly originalLineNumber?: number;

  constructor(
    reason: AttributeParseErrorReason,
    lineNumber: number,
    message: string,
    originalLineNumber?: number,
  ) {
    super(message);
    this.name = "AttributeParseError";
    this.reason = reason;
    this.lineNumber = lineNumber;
    this.originalLineNumber = originalLineNumber;
  }
}

export function parseAttributes(input: string): Record<string, string> {
  const entries: Array<[string, string]> = [];
  const keyLines = new Map<string, number>();

  for (const [index, line] of input.split("\n").entries()) {
    const lineNumber = index + 1;
    if (line.trim() === "") continue;

    const delimiter = line.indexOf("=");
    if (delimiter < 0) {
      throw new AttributeParseError(
        "malformed-line",
        lineNumber,
        `Line ${lineNumber} must use key=value.`,
      );
    }

    const key = line.slice(0, delimiter).trim();
    const value = line.slice(delimiter + 1).trim();
    if (key === "") {
      throw new AttributeParseError(
        "empty-key",
        lineNumber,
        `Line ${lineNumber} has an empty attribute key.`,
      );
    }

    const originalLineNumber = keyLines.get(key);
    if (originalLineNumber !== undefined) {
      throw new AttributeParseError(
        "duplicate-key",
        lineNumber,
        `Line ${lineNumber} repeats attribute key "${key}" from line ${originalLineNumber}.`,
        originalLineNumber,
      );
    }

    keyLines.set(key, lineNumber);
    entries.push([key, value]);
  }

  return Object.fromEntries(entries);
}
