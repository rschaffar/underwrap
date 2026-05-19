import { afterEach, describe, expect, it } from "vitest";
import { render } from "@testing-library/react";

import {
  ModeBadge,
  deriveClientInstances,
  deriveWrapperResources,
  modeKind,
  wrapperResourceKey,
} from "./matrix";
import type { AgentGrant, WrapperApprovalMode } from "@/api/client";

afterEach(() => {});

describe("derivation helpers", () => {
  const grants: AgentGrant[] = [
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
  const catalog = {
    entries: [
      {
        client_instance: "dev-laptop",
        wrapper: "gh-authorized",
        resource: "github/token",
      },
      {
        client_instance: "dev-laptop",
        wrapper: "jira-readonly",
        resource: "jira/api",
      },
    ],
  };

  it("deriveClientInstances dedupes and sorts", () => {
    const instances = deriveClientInstances(catalog, grants);
    expect(instances).toEqual(["ci-runner", "dev-laptop"]);
  });

  it("deriveWrapperResources dedupes across catalog and grants", () => {
    const pairs = deriveWrapperResources(catalog, grants);
    expect(pairs.map(wrapperResourceKey)).toEqual([
      "aws-prod|aws/prod",
      "gh-authorized|github/token",
      "jira-readonly|jira/api",
    ]);
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
