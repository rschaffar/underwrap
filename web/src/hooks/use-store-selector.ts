import { useEffect, useState } from "react";

/**
 * URL-persisted store selector shared by the per-store admin pages
 * (Secrets, Wrapper permissions). Reads `?store=` once on mount,
 * defaults to the first available store when none is in the URL,
 * and writes the chosen store back to the URL via
 * `history.replaceState`.
 */
export function useStoreSelector(
  availableStores: ReadonlyArray<{ id: string }> | undefined,
): { storeId: string | null; setStoreId: (id: string) => void } {
  const [storeId, setStoreId] = useState<string | null>(() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("store");
  });

  useEffect(() => {
    if (storeId) return;
    const first = availableStores?.[0]?.id;
    if (first) setStoreId(first);
  }, [storeId, availableStores]);

  useEffect(() => {
    if (!storeId) return;
    const url = new URL(window.location.href);
    url.searchParams.set("store", storeId);
    window.history.replaceState({}, "", url.toString());
  }, [storeId]);

  return { storeId, setStoreId };
}
