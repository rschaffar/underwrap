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
): { storeName: string | null; setStoreName: (id: string) => void } {
  const [storeName, setStoreName] = useState<string | null>(() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("store");
  });

  useEffect(() => {
    if (storeName) return;
    const first = availableStores?.[0]?.id;
    if (first) setStoreName(first);
  }, [storeName, availableStores]);

  useEffect(() => {
    if (!storeName) return;
    const url = new URL(window.location.href);
    url.searchParams.set("store", storeName);
    window.history.replaceState({}, "", url.toString());
  }, [storeName]);

  return { storeName, setStoreName };
}
