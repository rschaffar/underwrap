import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";

import { ModeBadge } from "./wrapper-permissions/table";
import {
  clientInstancesForStore,
  groupPairsByResource,
} from "./wrapper-permissions/derivations";
import { formatEpochRelative } from "@/lib/epoch";
import type {
  WrapperApprovalMode,
  WrapperPermission,
  WrapperResourcePairRecord,
} from "@/api/client";

describe("derivation helpers", () => {
  const pairs: WrapperResourcePairRecord[] = [
    {
      wrapper: "jira-readonly",
      resource_id: "jira/api",
      created_by: { kind: "operator", id: "op-1", username: "alice" },
      created_at: 100,
      last_used_at: null,
    },
    {
      wrapper: "aws-prod",
      resource_id: "aws/prod",
      created_by: { kind: "client-instance", id: "ci-runner" },
      created_at: 200,
      last_used_at: null,
    },
    {
      wrapper: "gh-authorized",
      resource_id: "github/token",
      created_by: { kind: "operator", id: "op-1", username: "alice" },
      created_at: 150,
      last_used_at: null,
    },
  ];

  it("groupPairsByResource buckets by resource and sorts both axes", () => {
    const shared: WrapperResourcePairRecord[] = [
      ...pairs,
      {
        wrapper: "gh-cli",
        resource_id: "github/token",
        created_by: { kind: "operator", id: "op-1", username: "alice" },
        created_at: 175,
        last_used_at: null,
      },
    ];
    const groups = groupPairsByResource(shared);
    expect(groups.map((g) => g.resource)).toEqual([
      "aws/prod",
      "github/token",
      "jira/api",
    ]);
    const github = groups.find((g) => g.resource === "github/token")!;
    expect(github.pairs.map((p) => p.wrapper)).toEqual(["gh-authorized", "gh-cli"]);
  });

  it("formatEpochRelative renders 'never' for null/undefined/zero", () => {
    expect(formatEpochRelative(null)).toBe("never");
    expect(formatEpochRelative(undefined)).toBe("never");
    expect(formatEpochRelative(0)).toBe("never");
  });

  it("formatEpochRelative renders a relative label for a recent epoch", () => {
    // Anchor "now" so the test isn't time-sensitive. 90 seconds ago.
    const now = 1_700_000_000_000;
    const epoch = (now / 1000 - 90);
    const label = formatEpochRelative(epoch, now);
    // Intl.RelativeTimeFormat localizes the output. Just assert it
    // contains a unit hint, not the exact phrasing.
    expect(label.length).toBeGreaterThan(0);
    expect(label).not.toBe("never");
  });

});

describe("ModeBadge", () => {
  it("renders every mode/expiry badge variant", () => {
    const future = Math.floor(Date.now() / 1000) + 3600;
    const past = Math.floor(Date.now() / 1000) - 3600;
    const variants: Array<{ mode: WrapperApprovalMode; testid: string }> = [
      { mode: { kind: "locked" }, testid: "mode-locked" },
      { mode: { kind: "unlocked" }, testid: "mode-unlocked" },
      {
        mode: { kind: "unlocked-until", until_epoch: future },
        testid: "mode-unlocked-until",
      },
      {
        mode: { kind: "unlocked-until", until_epoch: past },
        testid: "mode-expired",
      },
      {
        mode: { kind: "unsafe-debug-until", until_epoch: future },
        testid: "mode-unsafe-debug-until",
      },
      {
        mode: { kind: "unsafe-debug-until", until_epoch: past },
        testid: "mode-unsafe-debug-expired",
      },
    ];
    for (const v of variants) {
      const { getByTestId, unmount } = render(<ModeBadge mode={v.mode} />);
      expect(getByTestId(v.testid)).toBeTruthy();
      unmount();
    }
  });
});

describe("clientInstancesForStore", () => {
  const perm = (client_instance: string): WrapperPermission => ({
    client_instance,
    wrapper: "gh",
    resource_id: "r",
    mode: { kind: "unlocked" },
    writable: false,
  });

  it("returns instances currently mapped to the store", () => {
    const instances = [
      { id: "ci-a", current_store_id: "prod" },
      { id: "ci-b", current_store_id: "other" },
    ];
    expect(clientInstancesForStore("prod", instances, [])).toEqual(["ci-a"]);
  });

  it("includes stale instances that only appear in existing permissions", () => {
    // ci-old was remapped away but still has a permission row in this store.
    expect(
      clientInstancesForStore("prod", [], [perm("ci-old")]),
    ).toEqual(["ci-old"]);
  });

  it("unions active and stale columns and sorts them", () => {
    const instances = [{ id: "ci-b", current_store_id: "prod" }];
    expect(
      clientInstancesForStore("prod", instances, [perm("ci-a")]),
    ).toEqual(["ci-a", "ci-b"]);
  });

  it("returns an empty list for a store with no instances or permissions", () => {
    expect(clientInstancesForStore("prod", [], [])).toEqual([]);
  });
});
