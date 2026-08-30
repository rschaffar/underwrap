import { StrictMode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useStoreSelector } from "./use-store-selector";

const STORES = [{ id: "primary" }, { id: "secondary" }] as const;

function Harness({
  stores,
}: {
  stores: ReadonlyArray<{ id: string }> | undefined;
}) {
  const { storeId, setStoreId } = useStoreSelector(stores);
  return (
    <>
      <output data-testid="effective-store">{storeId ?? "none"}</output>
      <button type="button" onClick={() => setStoreId("primary")}>
        Select primary
      </button>
      <button type="button" onClick={() => setStoreId("secondary")}>
        Select secondary
      </button>
      <button type="button" onClick={() => setStoreId("missing")}>
        Select missing
      </button>
    </>
  );
}

function renderSelector(stores: ReadonlyArray<{ id: string }> | undefined) {
  return render(
    <StrictMode>
      <Harness stores={stores} />
    </StrictMode>,
  );
}

function setLocation(path: string, state: unknown = null) {
  window.history.replaceState(state, "", path);
}

function effectiveStore(): string {
  return screen.getByTestId("effective-store").textContent ?? "";
}

afterEach(() => {
  vi.restoreAllMocks();
  setLocation("/");
});

describe("useStoreSelector", () => {
  it("never exposes an unresolved stale id and canonicalizes one fallback exactly once", () => {
    setLocation("/secrets?store=stale&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");
    const view = renderSelector(undefined);

    expect(effectiveStore()).toBe("none");
    expect(replaceState).not.toHaveBeenCalled();

    view.rerender(
      <StrictMode>
        <Harness stores={STORES} />
      </StrictMode>,
    );

    expect(effectiveStore()).toBe("primary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/secrets?store=primary&keep=yes#resource",
    );
    expect(replaceState).toHaveBeenCalledTimes(1);
  });

  it("falls back after removal and clears only store after the resolved list becomes empty", () => {
    setLocation("/secrets?store=secondary&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");
    const view = renderSelector(STORES);

    expect(effectiveStore()).toBe("secondary");
    expect(replaceState).not.toHaveBeenCalled();

    view.rerender(
      <StrictMode>
        <Harness stores={[STORES[0]]} />
      </StrictMode>,
    );
    expect(effectiveStore()).toBe("primary");
    expect(window.location.search).toBe("?store=primary&keep=yes");

    view.rerender(
      <StrictMode>
        <Harness stores={[]} />
      </StrictMode>,
    );
    expect(effectiveStore()).toBe("none");
    expect(window.location.search).toBe("?keep=yes");
    expect(window.location.hash).toBe("#resource");
    expect(replaceState).toHaveBeenCalledTimes(2);
  });

  it("updates a valid selection immediately without adding history or rewriting an effective choice", () => {
    setLocation("/wrappers?store=secondary&keep=yes#policy", { marker: "selection" });
    const initialLength = window.history.length;
    const replaceState = vi.spyOn(window.history, "replaceState");
    renderSelector(STORES);

    expect(effectiveStore()).toBe("secondary");
    expect(replaceState).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Select primary" }));
    expect(effectiveStore()).toBe("primary");
    expect(window.location.search).toBe("?store=primary&keep=yes");
    expect(window.location.hash).toBe("#policy");
    expect(window.history.length).toBe(initialLength);
    expect(replaceState).toHaveBeenCalledTimes(1);

    replaceState.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Select primary" }));
    expect(effectiveStore()).toBe("primary");
    expect(replaceState).not.toHaveBeenCalled();
  });

  it("preserves exact history state, unrelated query, hash, and history length when defaulting", () => {
    setLocation("/secrets?keep=yes#resource", {
      marker: "exact-state",
      nested: { count: 1 },
    });
    const exactState = window.history.state;
    const initialLength = window.history.length;
    const replaceState = vi.spyOn(window.history, "replaceState");

    renderSelector(STORES);

    expect(effectiveStore()).toBe("primary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/secrets?keep=yes&store=primary#resource",
    );
    expect(window.history.state).toBe(exactState);
    expect(window.history.length).toBe(initialLength);
    expect(replaceState).toHaveBeenCalledTimes(1);
    expect(replaceState.mock.calls[0]?.[0]).toBe(exactState);
  });

  it("does not replace a canonical initial URL or an already-effective setter choice", () => {
    setLocation("/secrets?store=primary&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");

    renderSelector(STORES);
    expect(effectiveStore()).toBe("primary");
    expect(replaceState).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Select primary" }));
    expect(replaceState).not.toHaveBeenCalled();
  });

  it("rejects a setter id absent from the resolved store list without changing state or history", () => {
    const state = { marker: "invalid-setter" };
    setLocation("/secrets?store=secondary&keep=yes#resource", state);
    const initialLength = window.history.length;
    const replaceState = vi.spyOn(window.history, "replaceState");
    renderSelector(STORES);

    fireEvent.click(screen.getByRole("button", { name: "Select missing" }));

    expect(effectiveStore()).toBe("secondary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/secrets?store=secondary&keep=yes#resource",
    );
    expect(window.history.state).toBe(state);
    expect(window.history.length).toBe(initialLength);
    expect(replaceState).not.toHaveBeenCalled();
  });

  it("rereads valid and stale popstate selections through normal validation", () => {
    setLocation("/secrets?store=primary&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");
    renderSelector(STORES);
    expect(replaceState).not.toHaveBeenCalled();

    setLocation("/secrets?store=secondary&keep=yes#resource", { nav: "valid" });
    replaceState.mockClear();
    act(() => window.dispatchEvent(new PopStateEvent("popstate")));
    expect(effectiveStore()).toBe("secondary");
    expect(replaceState).not.toHaveBeenCalled();

    setLocation("/secrets?store=stale&keep=yes#resource", { nav: "stale" });
    const staleState = window.history.state;
    replaceState.mockClear();
    act(() => window.dispatchEvent(new PopStateEvent("popstate")));

    expect(effectiveStore()).toBe("primary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/secrets?store=primary&keep=yes#resource",
    );
    expect(window.history.state).toBe(staleState);
    expect(replaceState).toHaveBeenCalledTimes(1);
  });

  it("canonicalizes same-path duplicate params once when popstate keeps the same first id", async () => {
    setLocation("/secrets?store=primary&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");
    renderSelector(STORES);

    setLocation(
      "/secrets?store=primary&store=secondary&keep=yes#resource",
      { nav: "duplicate" },
    );
    const exactState = window.history.state;
    replaceState.mockClear();
    act(() => window.dispatchEvent(new PopStateEvent("popstate")));

    expect(effectiveStore()).toBe("primary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/secrets?store=primary&keep=yes#resource",
    );
    expect(window.history.state).toBe(exactState);
    expect(replaceState).toHaveBeenCalledTimes(1);
    expect(replaceState.mock.calls[0]?.[0]).toBe(exactState);

    await act(async () => {});
    expect(replaceState).toHaveBeenCalledTimes(1);
  });

  it("ignores popstate after navigation changes the mounted pathname", () => {
    setLocation("/secrets?store=primary&keep=yes#resource");
    const replaceState = vi.spyOn(window.history, "replaceState");
    renderSelector(STORES);

    setLocation("/wrappers?store=secondary&keep=yes#policy", {
      nav: "different-path",
    });
    replaceState.mockClear();
    act(() => window.dispatchEvent(new PopStateEvent("popstate")));

    expect(effectiveStore()).toBe("primary");
    expect(window.location.pathname + window.location.search + window.location.hash).toBe(
      "/wrappers?store=secondary&keep=yes#policy",
    );
    expect(replaceState).not.toHaveBeenCalled();
  });
});
