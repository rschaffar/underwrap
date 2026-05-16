import { describe, expect, it } from "vitest";

import { describeApiError } from "./error-message";
import { ApiError } from "@/api/client";

describe("describeApiError", () => {
  it("maps each backend code to a human-readable hint", () => {
    const cases: Array<[string, RegExp]> = [
      ["invalid_credentials", /invalid username or password/i],
      ["rate_limited", /too many attempts/i],
      ["bootstrap_already_completed", /already been completed/i],
      ["operators_already_exist", /operator already exists/i],
      ["csrf_required", /session is stale/i],
      ["csrf_mismatch", /session is stale/i],
      ["session_revoked", /session was revoked/i],
      ["session_key_not_configured", /misconfigured/i],
    ];
    for (const [code, pattern] of cases) {
      const msg = describeApiError(new ApiError(500, code, "raw backend msg"));
      expect(msg).toMatch(pattern);
    }
  });

  it("falls through to ApiError.message for unknown codes", () => {
    const msg = describeApiError(new ApiError(500, "weird_new_code", "raw"));
    expect(msg).toBe("raw");
  });

  it("falls through to a generic blurb when message is empty", () => {
    const msg = describeApiError(new ApiError(500, "weird_new_code", ""));
    expect(msg).toMatch(/request failed/i);
  });

  it("handles non-ApiError values (e.g. a TypeError from fetch)", () => {
    expect(describeApiError(new TypeError("network"))).toMatch(
      /network error/i,
    );
    expect(describeApiError("string error")).toMatch(/network error/i);
    expect(describeApiError(null)).toMatch(/network error/i);
  });
});
