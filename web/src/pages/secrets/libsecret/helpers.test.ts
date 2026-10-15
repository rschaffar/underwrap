import { describe, expect, it } from "vitest";

import {
  AttributeParseError,
  libsecretIdField,
  optionalLibsecretIdField,
  optionalLibsecretItemIdField,
  parseAttributes,
} from "./helpers";

describe("libsecret identifiers", () => {
  it.each(["_", "0", "Ab_09", "a".repeat(128)])("accepts segment %s exactly", (id) => {
    expect(libsecretIdField().parse(id)).toBe(id);
    expect(optionalLibsecretIdField().parse(id)).toBe(id);
  });

  it.each(["a.b", "a-b", ".", "..", "a/b", "a b", "é", "a".repeat(129), " id", "id ", "id\n", "id\r\n"])(
    "rejects an invalid segment without normalization: %j", (id) => {
      expect(libsecretIdField().safeParse(id).success).toBe(false);
      expect(optionalLibsecretIdField().safeParse(id).success).toBe(false);
    },
  );

  it("allows only blank or omitted optional ids to select generation", () => {
    expect(libsecretIdField().safeParse("").success).toBe(false);
    for (const field of [optionalLibsecretIdField(), optionalLibsecretItemIdField()]) {
      expect(field.parse("")).toBe("");
      expect(field.parse(undefined)).toBeUndefined();
      expect(field.safeParse(" ").success).toBe(false);
    }
  });

  it.each(["0".repeat(32), "abcdef0123456789".repeat(2)])("accepts exact item id %s", (id) => {
    expect(optionalLibsecretItemIdField().parse(id)).toBe(id);
  });

  it.each(["item_1", "a".repeat(31), "a".repeat(33), "A".repeat(32), "g".repeat(32), "_".repeat(32), "é".repeat(16), "a".repeat(32) + "\n", " " + "a".repeat(32)])(
    "rejects invalid item id without normalization: %j", (id) => {
      expect(optionalLibsecretItemIdField().safeParse(id).success).toBe(false);
    },
  );
});

function parseError(input: string): AttributeParseError {
  try {
    parseAttributes(input);
  } catch (error) {
    expect(error).toBeInstanceOf(AttributeParseError);
    return error as AttributeParseError;
  }
  throw new Error("expected attribute parsing to fail");
}

describe("parseAttributes", () => {
  it("preserves trimming, blank lines, CRLF, empty values, first delimiters, and case", () => {
    expect(
      parseAttributes(
        " service = github.com \r\n\r\nempty=   \r\nquery = alpha=beta=gamma\r\nService = distinct \r\n",
      ),
    ).toEqual({
      service: "github.com",
      empty: "",
      query: "alpha=beta=gamma",
      Service: "distinct",
    });
  });

  it("rejects a nonblank line without a delimiter", () => {
    const error = parseError("first=value\n   \nmalformed");

    expect(error).toMatchObject({
      reason: "malformed-line",
      lineNumber: 3,
      originalLineNumber: undefined,
      message: "Line 3 must use key=value.",
    });
  });

  it("rejects an empty trimmed key", () => {
    const error = parseError("first=value\n \t = allowed-value");

    expect(error).toMatchObject({
      reason: "empty-key",
      lineNumber: 2,
      originalLineNumber: undefined,
      message: "Line 2 has an empty attribute key.",
    });
  });

  it("rejects a duplicate trimmed key with both source lines", () => {
    const error = parseError(" service = first\nService=distinct\nservice=second");

    expect(error).toMatchObject({
      reason: "duplicate-key",
      lineNumber: 3,
      originalLineNumber: 1,
      message: 'Line 3 repeats attribute key "service" from line 1.',
    });
  });

  it("represents object-like keys as ordinary own string properties", () => {
    const attributes = parseAttributes(
      "__proto__=proto\nconstructor=ctor\ntoString=stringifier",
    );

    expect(Object.getPrototypeOf(attributes)).toBe(Object.prototype);
    expect(Object.keys(attributes)).toEqual([
      "__proto__",
      "constructor",
      "toString",
    ]);
    expect(Object.hasOwn(attributes, "__proto__")).toBe(true);
    expect(Object.hasOwn(attributes, "constructor")).toBe(true);
    expect(Object.hasOwn(attributes, "toString")).toBe(true);
    expect(attributes["__proto__"]).toBe("proto");
    expect(attributes["constructor"]).toBe("ctor");
    expect(attributes["toString"]).toBe("stringifier");
    expect(Object.getPrototypeOf({})).toBe(Object.prototype);
  });
});
