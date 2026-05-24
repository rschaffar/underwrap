import { useCallback, useEffect, useState } from "react";

/**
 * URL-persisted store selector shared by the per-store admin pages
 * (Secrets, Wrapper permissions). Reads `?store=` once on mount,
 * defaults to the first available store when none is in the URL,
 * and writes the chosen store back to the URL inside the setter.
 */
export function useStoreSelector(
  availableStores: ReadonlyArray<{ id: string }> | undefined,
): { storeId: string | null; setStoreId: (id: string) => void } {
  const [storeId, setStoreIdState] = useState<string | null>(() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("store");
  });

  const setStoreId = useCallback((id: string) => {
    setStoreIdState(id);
    const url = new URL(window.location.href);
    url.searchParams.set("store", id);
    window.history.replaceState({}, "", url.toString());
  }, []);

  useEffect(() => {
    if (storeId) return;
    const first = availableStores?.[0]?.id;
    if (first) setStoreId(first);
  }, [storeId, availableStores, setStoreId]);

  return { storeId, setStoreId };
}
