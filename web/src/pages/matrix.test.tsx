import { afterEach, describe, expect, it } from "vitest";
import { render } from "@testing-library/react";

import {
  ModeBadge,
  derivePrincipals,
  deriveWrappers,
  modeKind,
  principalKey,
} from "./matrix";
import type { AgentGrant, WrapperApprovalMode } from "@/api/client";

afterEach(() => {});

describe("derivation helpers", () => {
  const grants: AgentGrant[] = [
    {
      wrapper: "gh-authorized",
      host: "deepthought",
      user: "robert",
      mode: { kind: "unlocked" } as WrapperApprovalMode,
      reason: undefined,
    },
    {
      wrapper: "aws-prod",
      host: "ci",
      user: "bot",
      mode: { kind: "locked" } as WrapperApprovalMode,
      reason: undefined,
    },
  ];
  const catalog = {
    entries: [
      {
        host: "deepthought",
        user: "robert",
        instance: "",
        wrapper: "gh-authorized",
        resource: "github/token",
      },
      {
        host: "deepthought",
        user: "robert",
        instance: "",
        wrapper: "jira-readonly",
        resource: "jira/api",
      },
    ],
  };

  it("derivePrincipals dedupes and sorts by (host, user)", () => {
    const principals = derivePrincipals(catalog, grants);
    expect(principals.map(principalKey)).toEqual([
      "ci/bot",
      "deepthought/robert",
    ]);
  });

  it("deriveWrappers dedupes across catalog and grants", () => {
    const wrappers = deriveWrappers(catalog, grants);
    expect(wrappers).toEqual([
      "aws-prod",
      "gh-authorized",
      "jira-readonly",
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
