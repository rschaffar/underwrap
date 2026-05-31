import { describe, expect, it } from "vitest";

import type { AdminSecretEntry } from "@/api/client";

import { groupSecretEntries, resourceCount } from "./grouping";

const entry = (
  resource_id: string,
  slot: "env" | "files",
  key: string,
): AdminSecretEntry => ({ resource_id, slot, key });

describe("groupSecretEntries", () => {
  it("buckets entries by resource and slot", () => {
    const groups = groupSecretEntries([
      entry("github/token", "env", "GH_TOKEN"),
      entry("github/token", "files", "config.yaml"),
      entry("aws/prod", "env", "AWS_KEY"),
    ]);
    expect(groups.map((g) => g.resource_id)).toEqual([
      "aws/prod",
      "github/token",
    ]);
    const gh = groups.find((g) => g.resource_id === "github/token")!;
    expect(gh.env.map((e) => e.key)).toEqual(["GH_TOKEN"]);
    expect(gh.files.map((e) => e.key)).toEqual(["config.yaml"]);
  });

  it("sorts each slot by key and resources by id", () => {
    const groups = groupSecretEntries([
      entry("svc", "env", "ZED"),
      entry("svc", "env", "ALPHA"),
      entry("svc", "files", "z.txt"),
      entry("svc", "files", "a.txt"),
    ]);
    expect(groups[0].env.map((e) => e.key)).toEqual(["ALPHA", "ZED"]);
    expect(groups[0].files.map((e) => e.key)).toEqual(["a.txt", "z.txt"]);
  });

  it("returns an empty array for no entries", () => {
    expect(groupSecretEntries([])).toEqual([]);
  });
});

describe("resourceCount", () => {
  it("sums the flat env and files slots", () => {
    expect(
      resourceCount({
        resource_id: "r",
        env: [{ slot: "env", key: "A" }],
        files: [
          { slot: "files", key: "x" },
          { slot: "files", key: "y" },
        ],
      }),
    ).toBe(3);
  });
});
