import { useEffect, useRef } from "react";
import { useRouter, useRouterState } from "@tanstack/react-router";

/** Consume a navigation request to open a dialog; never submit a mutation. */
export function usePageIntent(
  key: "create" | "unlock",
  ready: boolean,
  open: (value: string) => void,
) {
  const router = useRouter();
  const { location, status } = useRouterState({
    select: (state) => ({ location: state.location, status: state.status }),
    structuralSharing: false,
  });
  const handled = useRef<string | null>(null);
  const value = location.search[key];
  useEffect(() => {
    if (typeof value !== "string") {
      handled.current = null;
      return;
    }
    if (
      !ready ||
      handled.current === location.href ||
      router.state.status !== "idle" ||
      router.state.location !== location ||
      router.latestLocation.href !== location.href
    )
      return;
    handled.current = location.href;
    open(value);
    void router.navigate({
      to: location.pathname,
      search: (previous) => {
        const next = { ...previous };
        delete next[key];
        return next;
      },
      replace: true,
      hash: true,
      state: true,
      resetScroll: false,
    });
  }, [key, location, open, ready, router, status, value]);
}
