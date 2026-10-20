import { describe, expect, it } from "vitest";

import { decodeBase64Utf8, encodeUtf8Base64 } from "./utf8-base64";

describe("UTF-8 base64 conversion", () => {
  it.each([
    ["✓ à la mode", "4pyTIMOgIGxhIG1vZGU="],
    ["päss🔐", "cMOkc3Pwn5SQ"],
    ["日本語", "5pel5pys6Kqe"],
    ["", ""],
  ])("encodes and decodes %j exactly", (text, base64) => {
    expect(encodeUtf8Base64(text)).toBe(base64);
    expect(decodeBase64Utf8(base64)).toEqual({ ok: true, text });
  });

  it.each([
    ["BOM only", "\uFEFF"],
    ["leading BOM and Unicode", "\uFEFF日本語🔐"],
    ["repeated leading BOM", "\uFEFF\uFEFFtext"],
    ["interior BOM", "text\uFEFFtext"],
  ])("preserves exact code points: %s", (_label, text) => {
    const decoded = decodeBase64Utf8(encodeUtf8Base64(text));

    // Compare in memory so a failing assertion does not print material.
    expect(decoded.ok && decoded.text === text).toBe(true);
  });

  it("round-trips multi-byte text larger than one conversion chunk", () => {
    const text = "日本語🔐".repeat(10_000);
    const encoded = encodeUtf8Base64(text);

    expect(decodeBase64Utf8(encoded)).toEqual({ ok: true, text });
  });

  it("distinguishes malformed base64 from invalid UTF-8", () => {
    expect(decodeBase64Utf8("%%%")).toEqual({
      ok: false,
      reason: "invalid-base64",
    });
    for (const bytes of [
      [0xff, 0xfe],
      [0xef, 0xbb], // Truncated BOM is still invalid UTF-8.
      [0xef, 0xbb, 0xbf, 0xff], // A valid BOM must not hide an invalid suffix.
    ]) {
      const decoded = decodeBase64Utf8(btoa(String.fromCharCode(...bytes)));
      expect(!decoded.ok && decoded.reason === "invalid-utf8").toBe(true);
    }
  });
});
