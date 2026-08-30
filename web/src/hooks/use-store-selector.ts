import { useCallback, useEffect, useReducer, useRef } from "react";

function requestedStoreId(): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get("store");
}

function replaceRequestedStoreId(id: string | null): void {
  const current = new URL(window.location.href);
  const currentIds = current.searchParams.getAll("store");
  const alreadyCanonical =
    id === null
      ? currentIds.length === 0
      : currentIds.length === 1 && currentIds[0] === id;
  if (alreadyCanonical) return;

  const next = new URL(window.location.href);
  if (id === null) {
    next.searchParams.delete("store");
  } else {
    next.searchParams.set("store", id);
  }

  const currentPath = current.pathname + current.search + current.hash;
  const nextPath = next.pathname + next.search + next.hash;
  if (nextPath !== currentPath) {
    window.history.replaceState(window.history.state, "", nextPath);
  }
}

/**
 * URL-persisted store selector shared by the per-store admin pages.
 * The URL value remains requested state until a resolved store list
 * synchronously validates it into the effective returned selection.
 */
export function useStoreSelector(
  availableStores: ReadonlyArray<{ id: string }> | undefined,
): { storeId: string | null; setStoreId: (id: string) => void } {
  const [{ id: requestedId, revision }, updateRequestedId] = useReducer(
    (state: { id: string | null; revision: number }, id: string | null) => ({
      id,
      revision: state.revision + 1,
    }),
    undefined,
    () => ({ id: requestedStoreId(), revision: 0 }),
  );
  const mountedPathname = useRef(
    typeof window === "undefined" ? null : window.location.pathname,
  ).current;

  const storeId =
    availableStores === undefined
      ? null
      : requestedId !== null &&
          availableStores.some((store) => store.id === requestedId)
        ? requestedId
        : (availableStores[0]?.id ?? null);

  const setStoreId = useCallback(
    (id: string) => {
      if (
        availableStores === undefined ||
        !availableStores.some((store) => store.id === id) ||
        id === storeId
      ) {
        return;
      }
      updateRequestedId(id);
      replaceRequestedStoreId(id);
    },
    [availableStores, storeId],
  );

  useEffect(() => {
    if (availableStores === undefined) return;
    if (requestedId !== storeId) {
      updateRequestedId(storeId);
    }
    replaceRequestedStoreId(storeId);
  }, [availableStores, requestedId, revision, storeId]);

  useEffect(() => {
    const onPopState = () => {
      if (window.location.pathname !== mountedPathname) return;
      updateRequestedId(requestedStoreId());
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, [mountedPathname]);

  return { storeId, setStoreId };
}
