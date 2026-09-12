import { Link } from "@tanstack/react-router";
import type { AdminStoreListItem } from "@/api/client";
import { Alert } from "@/components/ui/alert";
import { RECOVERY_GUIDE } from "@/lib/operator-guidance";

export function storeIsAvailable(
  store: AdminStoreListItem | undefined,
): boolean {
  return (
    store?.lifecycle_state === "ready" || store?.lifecycle_state === "unlocked"
  );
}

export function StoreAvailability({
  store,
}: {
  store: AdminStoreListItem | undefined;
}) {
  if (!store || storeIsAvailable(store)) return null;
  return (
    <Alert variant="warning">
      <strong>{store.name}</strong> is {store.lifecycle_state}.
      {store.lifecycle_state === "locked" ? (
        <>
          {" "}
          <Link
            className="underline"
            to="/stores"
            search={{ store: store.id, unlock: store.id }}
          >
            Unlock this store
          </Link>{" "}
          to access its resources and permission rows.
        </>
      ) : store.lifecycle_state === "recovery-required" ? (
        <>
          {" "}
          A trusted backend-host operator must follow the{" "}
          <a
            className="underline"
            href={RECOVERY_GUIDE}
            target="_blank"
            rel="noreferrer noopener"
          >
            recovery procedure
          </a>
          . Do not retry lifecycle operations blindly.
        </>
      ) : (
        <>
          {" "}
          Wait for the lifecycle operation to finish, then refresh the store
          list.
        </>
      )}{" "}
      Client overrides do not bypass store availability.
    </Alert>
  );
}
