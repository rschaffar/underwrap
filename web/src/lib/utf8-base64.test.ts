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
    expect(decodeBase64Utf8("//4=")).toEqual({
      ok: false,
      reason: "invalid-utf8",
    });
  });
});
