import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { useNavigationLock } from "./use-navigation-lock";
import { withTestRouter } from "@/test-router";

function unloadAttempt() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event;
}

function Guard({ clicked }: { clicked: () => void }) {
  const [active, setActive] = useState(true);
  useNavigationLock(active);
  return (
    <>
      <button onClick={() => setActive(false)}>Release navigation</button>
      <Link
        to="/profile"
        search={{ store: "secondary" }}
        hash="metadata"
        onClick={clicked}
      >
        Destination
      </Link>
    </>
  );
}

function renderGuard(clicked = vi.fn()) {
  window.history.replaceState({}, "", "/stores?store=secondary#metadata");
  return render(
    withTestRouter(<Guard clicked={clicked} />, {
      "/profile": <h1>Destination page</h1>,
    }),
  );
}

describe("useNavigationLock", () => {
  it("blocks real router navigation while active, then permits a fresh attempt", async () => {
    const user = userEvent.setup();
    const clicked = vi.fn();
    renderGuard(clicked);
    const link = await screen.findByRole("link", { name: "Destination" });
    expect(unloadAttempt().defaultPrevented).toBe(true);
    await user.click(link);
    expect(clicked).toHaveBeenCalledOnce();
    expect(window.location.pathname).toBe("/stores");
    expect(window.location.search).toBe("?store=secondary");
    expect(window.location.hash).toBe("#metadata");
    expect(
      screen.queryByRole("heading", { name: "Destination page" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Release navigation" }),
    );
    expect(unloadAttempt().defaultPrevented).toBe(false);
    expect(window.location.pathname).toBe("/stores");
    await user.click(link);
    await screen.findByRole("heading", { name: "Destination page" });
    await waitFor(() => expect(window.location.pathname).toBe("/profile"));
    expect(window.location.search).toBe("?store=secondary");
    expect(window.location.hash).toBe("#metadata");
    expect(clicked).toHaveBeenCalledTimes(2);
  });

  it("removes its unload handler when an active owner unmounts", async () => {
    const view = renderGuard();
    await screen.findByRole("link", { name: "Destination" });
    expect(unloadAttempt().defaultPrevented).toBe(true);
    view.unmount();
    expect(unloadAttempt().defaultPrevented).toBe(false);
  });
});
