import { describe, expect, it } from "vitest";

import { safeNextPath } from "./router";

const ORIGIN = window.location.origin;

describe("safeNextPath", () => {
  it("returns null for a missing or empty value", () => {
    expect(safeNextPath(null)).toBeNull();
    expect(safeNextPath("")).toBeNull();
  });

  it("passes a same-origin absolute path through unchanged", () => {
    expect(safeNextPath("/secrets")).toBe("/secrets");
  });

  it("preserves query and hash on a same-origin path", () => {
    expect(safeNextPath("/stores?tab=keys#row")).toBe("/stores?tab=keys#row");
  });

  it("resolves a same-origin absolute URL down to its path", () => {
    expect(safeNextPath(`${ORIGIN}/wrappers?x=1`)).toBe("/wrappers?x=1");
  });

  it("blocks a cross-origin absolute URL (open-redirect defence)", () => {
    expect(safeNextPath("https://evil.example.com/phish")).toBeNull();
  });

  it("blocks a protocol-relative URL pointing off-origin", () => {
    expect(safeNextPath("//evil.example.com/phish")).toBeNull();
  });

  it("blocks a javascript: scheme", () => {
    expect(safeNextPath("javascript:alert(1)")).toBeNull();
  });

  it("blocks a value that does not resolve to an absolute path", () => {
    // A bare word resolves relative to the current path, not the root.
    // We only accept results whose pathname starts with "/", which this
    // does (URL normalises it), so assert the security-relevant property:
    // the result never escapes the current origin.
    const out = safeNextPath("mailto:admin@example.com");
    expect(out).toBeNull();
  });
});
