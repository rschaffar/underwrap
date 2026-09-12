import { useCallback, useEffect } from "react";
import { useMatch, useRouter, useRouterState } from "@tanstack/react-router";

/**
 * The router URL is requested state; only a resolved store list makes it usable.
 * Do not mirror the URL in component state or write native history behind the router.
 */
export function useStoreSelector(
  availableStores: ReadonlyArray<{ id: string }> | undefined,
): { storeId: string | null; setStoreId: (id: string) => void } {
  const router = useRouter();
  const ownerMatchId = useMatch({
    strict: false,
    shouldThrow: false,
    select: (match) => match.id,
  });
  const { location, status } = useRouterState({
    select: (state) => ({ location: state.location, status: state.status }),
    structuralSharing: false,
  });
  // TanStack parses repeated keys as arrays and re-serializes searchStr.
  // Use its parsed value for selection and its history URL for raw duplicates.
  const historyHref = router.history.location.href;
  const rawIds = new URLSearchParams(router.history.location.search).getAll(
    "store",
  );
  const requested = location.search.store;
  const first = Array.isArray(requested) ? requested[0] : requested;
  const requestedId =
    typeof first === "string" ||
    typeof first === "number" ||
    typeof first === "boolean"
      ? String(first)
      : null;
  const storeId =
    availableStores === undefined
      ? null
      : availableStores.some((store) => store.id === requestedId)
        ? requestedId
        : (availableStores[0]?.id ?? null);
  const canonical =
    storeId === null
      ? rawIds.length === 0
      : rawIds.length === 1 &&
        !Array.isArray(requested) &&
        requestedId === storeId;

  const replaceSelection = useCallback(
    (id: string | null) => {
      // Pending navigation belongs to its destination, not a departing page's
      // passive effect. Compare the captured location with both router views.
      if (
        ownerMatchId === undefined ||
        !router.state.matches.some((match) => match.id === ownerMatchId) ||
        router.state.status !== "idle" ||
        router.state.location !== location ||
        router.latestLocation.href !== location.href ||
        router.history.location.href !== historyHref
      ) {
        return;
      }
      if (id === storeId && canonical) return;
      // The Router owns both the navigation and serialization; unrelated
      // search values, hash and user history-state fields survive replacement.
      void router.navigate({
        to: location.pathname,
        search: (previous) => {
          const next: Record<string, unknown> & { store?: string } = {
            ...previous,
          };
          // A store-scoped form must not follow a selection change or fallback.
          if (
            id !== requestedId &&
            ["resource", "pair", "client"].includes(String(next.create))
          )
            delete next.create;
          if (id === null) delete next.store;
          else next.store = id;
          return next;
        },
        hash: true,
        replace: true,
        state: true,
        resetScroll: false,
        hashScrollIntoView: false,
      });
    },
    [
      canonical,
      historyHref,
      location,
      ownerMatchId,
      requestedId,
      router,
      storeId,
    ],
  );

  const setStoreId = useCallback(
    (id: string) => {
      if (!availableStores?.some((store) => store.id === id)) return;
      replaceSelection(id);
    },
    [availableStores, replaceSelection],
  );

  useEffect(() => {
    if (availableStores === undefined || canonical || status !== "idle") return;
    replaceSelection(storeId);
  }, [availableStores, canonical, replaceSelection, status, storeId]);

  return { storeId, setStoreId };
}
