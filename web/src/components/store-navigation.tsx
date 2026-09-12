import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";

import { fetchStores } from "@/api/client";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { AppShell } from "./app-shell";

/** Authenticated layout owner; lists metadata only, never resource contents. */
export function StoreNavigation({ children }: { children: ReactNode }) {
  const stores = useQuery({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  // A failed refresh must not advertise cached lifecycle metadata as current.
  const availableStores = stores.isSuccess ? stores.data.items : undefined;
  const { storeId } = useStoreSelector(availableStores);
  const store = availableStores?.find((item) => item.id === storeId);

  return (
    <AppShell
      storeId={storeId}
      store={store}
      storeStatus={
        stores.isError
          ? "error"
          : availableStores === undefined
            ? "pending"
            : "resolved"
      }
    >
      {children}
    </AppShell>
  );
}
