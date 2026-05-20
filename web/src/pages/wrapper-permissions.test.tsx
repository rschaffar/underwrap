import { afterEach, describe, expect, it } from "vitest";
import { render } from "@testing-library/react";

import {
  ModeBadge,
  deriveClientInstances,
  formatRelativeFromEpoch,
  modeKind,
  sortPairs,
  wrapperResourceKey,
} from "./wrapper-permissions";
import type {
  WrapperPermission,
  WrapperApprovalMode,
  WrapperResourcePairRecord,
} from "@/api/client";

afterEach(() => {});

describe("derivation helpers", () => {
  const permissions: WrapperPermission[] = [
    {
      client_instance: "dev-laptop",
      wrapper: "gh-authorized",
      resource: "github/token",
      mode: { kind: "unlocked" } as WrapperApprovalMode,
      writable: false,
      reason: undefined,
    },
    {
      client_instance: "ci-runner",
      wrapper: "aws-prod",
      resource: "aws/prod",
      mode: { kind: "locked" } as WrapperApprovalMode,
      writable: false,
      reason: undefined,
    },
  ];

  const pairs: WrapperResourcePairRecord[] = [
    {
      wrapper: "jira-readonly",
      resource: "jira/api",
      created_by: { kind: "operator", id: "op-1", username: "alice" },
      created_at: 100n,
      last_used_at: null,
      files_mount_paths: [],
    },
    {
      wrapper: "aws-prod",
      resource: "aws/prod",
      created_by: { kind: "client-instance", id: "ci-runner" },
      created_at: 200n,
      last_used_at: null,
      files_mount_paths: [],
    },
    {
      wrapper: "gh-authorized",
      resource: "github/token",
      created_by: { kind: "operator", id: "op-1", username: "alice" },
      created_at: 150n,
      last_used_at: null,
      files_mount_paths: [],
    },
  ];

  it("deriveClientInstances dedupes and sorts", () => {
    const instances = deriveClientInstances(permissions);
    expect(instances).toEqual(["ci-runner", "dev-laptop"]);
  });

  it("sortPairs orders by (wrapper, resource)", () => {
    const ordered = sortPairs(pairs);
    expect(ordered.map((p) => wrapperResourceKey(p))).toEqual([
      "aws-prod|aws/prod",
      "gh-authorized|github/token",
      "jira-readonly|jira/api",
    ]);
  });

  it("formatRelativeFromEpoch renders 'never' for null/undefined/zero", () => {
    expect(formatRelativeFromEpoch(null)).toBe("never");
    expect(formatRelativeFromEpoch(undefined)).toBe("never");
    expect(formatRelativeFromEpoch(0n)).toBe("never");
  });

  it("formatRelativeFromEpoch renders a relative label for a recent epoch", () => {
    // Anchor "now" so the test isn't time-sensitive. 90 seconds ago.
    const now = 1_700_000_000_000;
    const epoch = BigInt(now / 1000 - 90);
    const label = formatRelativeFromEpoch(epoch, now);
    // Intl.RelativeTimeFormat localizes the output. Just assert it
    // contains a unit hint, not the exact phrasing.
    expect(label.length).toBeGreaterThan(0);
    expect(label).not.toBe("never");
  });

  it("modeKind unwraps the tag for each variant", () => {
    expect(modeKind({ kind: "locked" } as WrapperApprovalMode)).toBe("locked");
    expect(modeKind({ kind: "unlocked" } as WrapperApprovalMode)).toBe(
      "unlocked",
    );
    expect(
      modeKind({
        kind: "unlocked-until",
        until_epoch: 1n,
      } as unknown as WrapperApprovalMode),
    ).toBe("unlocked-until");
    expect(
      modeKind({
        kind: "per-call-approval",
        approval_key: "k",
      } as WrapperApprovalMode),
    ).toBe("per-call-approval");
  });
});

describe("ModeBadge", () => {
  it("renders one of the four mode variants", () => {
    const variants: Array<{ mode: WrapperApprovalMode; testid: string }> = [
      { mode: { kind: "locked" } as WrapperApprovalMode, testid: "mode-locked" },
      {
        mode: { kind: "unlocked" } as WrapperApprovalMode,
        testid: "mode-unlocked",
      },
      {
        mode: {
          kind: "unlocked-until",
          until_epoch: 1_700_000_000,
        } as unknown as WrapperApprovalMode,
        testid: "mode-unlocked-until",
      },
      {
        mode: {
          kind: "per-call-approval",
          approval_key: "ssh-ed25519 AAAA",
        } as WrapperApprovalMode,
        testid: "mode-per-call-approval",
      },
    ];
    for (const v of variants) {
      const { getByTestId, unmount } = render(<ModeBadge mode={v.mode} />);
      expect(getByTestId(v.testid)).toBeTruthy();
      unmount();
    }
  });
});
