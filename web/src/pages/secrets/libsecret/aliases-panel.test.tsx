import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type { AdminLibsecretCollectionView } from "@/api/client";

import { AliasesPanel } from "./aliases-panel";

const defaultCollection: AdminLibsecretCollectionView = {
  id: "default",
  label: "Default",
  created: 0,
  modified: 0,
  items: [],
};

describe("AliasesPanel", () => {
  it("flags aliases whose target collection is missing as dangling", () => {
    render(
      <AliasesPanel
        aliases={{ default: "default", login: "missing" }}
        collections={[defaultCollection]}
        onCreate={() => {}}
        onDelete={() => {}}
      />,
    );

    const live = screen.getByTestId("libsecret-alias-default");
    const dangling = screen.getByTestId("libsecret-alias-login");
    expect(live).not.toHaveTextContent(/dangling/i);
    expect(dangling).toHaveTextContent(/dangling/i);
  });

  it("disables the set-alias button when no collections exist yet", () => {
    render(
      <AliasesPanel
        aliases={{}}
        collections={[]}
        onCreate={() => {}}
        onDelete={() => {}}
      />,
    );

    const panel = screen.getByTestId("libsecret-aliases");
    expect(panel).toHaveTextContent(/none/i);
    expect(within(panel).getByTestId("libsecret-set-alias")).toBeDisabled();
  });

  it("invokes onDelete with the alias name when the row's Delete is pressed", async () => {
    const onDelete = vi.fn();
    render(
      <AliasesPanel
        aliases={{ default: "default" }}
        collections={[defaultCollection]}
        onCreate={() => {}}
        onDelete={onDelete}
      />,
    );

    const row = screen.getByTestId("libsecret-alias-default");
    await userEvent.click(within(row).getByRole("button", { name: /delete/i }));
    expect(onDelete).toHaveBeenCalledWith("default");
  });
});
