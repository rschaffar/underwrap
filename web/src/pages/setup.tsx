import { Link, useRouter, useRouterState } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import {
  fetchClientInstances,
  fetchStores,
  fetchWrapperPermissionsView,
} from "@/api/client";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import {
  StoreAvailability,
  storeIsAvailable,
} from "@/components/store-availability";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { useStoreSelector } from "@/hooks/use-store-selector";
import { describeApiError } from "@/lib/error-message";
import { OPERATIONS_GUIDE } from "@/lib/operator-guidance";

export function SetupPage() {
  useDocumentTitle("underwrap — get a wrapper working");
  const router = useRouter();
  const location = useRouterState({
    select: (state) => state.location,
    structuralSharing: false,
  });
  const stores = useQuery({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });
  const { storeId, setStoreId } = useStoreSelector(
    stores.isSuccess ? stores.data.items : undefined,
  );
  const store = stores.data?.items.find((item) => item.id === storeId);
  const available = storeIsAvailable(store);
  const clients = useQuery({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });
  const mapped = clients.isSuccess
    ? clients.data.items.filter((item) => item.current_store_id === storeId)
    : [];
  const requestedClient =
    typeof location.search.client === "string"
      ? location.search.client
      : undefined;
  const client = requestedClient
    ? mapped.find((item) => item.id === requestedClient)
    : mapped[0];
  const setRequestedClient = (id: string) => {
    if (
      !mapped.some((item) => item.id === id) ||
      router.state.status !== "idle" ||
      router.state.location !== location ||
      router.latestLocation.href !== location.href
    )
      return;
    void router.navigate({
      to: "/setup",
      search: (previous) => ({ ...previous, client: id }),
      replace: true,
      hash: true,
      state: true,
      resetScroll: false,
    });
  };
  const view = useQuery({
    queryKey: ["admin", "wrapper-permissions-view", storeId],
    queryFn: () => fetchWrapperPermissionsView(storeId!),
    enabled: !!storeId && available,
  });
  const rawStore = Array.isArray(location.search.store)
    ? location.search.store[0]
    : location.search.store;
  // Unresolved URL context may travel, but it cannot enable scoped creation.
  const contextStore =
    storeId ??
    (!stores.isSuccess && typeof rawStore === "string" ? rawStore : undefined);
  const search = contextStore ? { store: contextStore } : {};
  return (
    <div className="max-w-4xl space-y-6">
      <header>
        <h2 className="text-xl font-semibold">Get a wrapper working</h2>
        <p className="mt-1 text-sm text-(--color-muted)">
          Connect backend setup with the required frontend-host steps. Nothing
          here automatically grants access.
        </p>
      </header>
      {stores.error ? <Alert>{describeApiError(stores.error)}</Alert> : null}
      {clients.error ? <Alert>{describeApiError(clients.error)}</Alert> : null}
      {view.error ? <Alert>{describeApiError(view.error)}</Alert> : null}
      <div className="flex flex-wrap gap-4">
        <div className="space-y-1">
          <Label htmlFor="setup-store">Store</Label>
          <Select
            id="setup-store"
            value={storeId ?? ""}
            onChange={(event) => setStoreId(event.target.value)}
            disabled={!stores.isSuccess || stores.data.items.length === 0}
          >
            {!storeId ? (
              <option value="">Select or create a store</option>
            ) : null}
            {stores.data?.items.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name}
              </option>
            ))}
          </Select>
        </div>
        <div className="space-y-1">
          <Label htmlFor="setup-client">Runtime client instance</Label>
          <Select
            id="setup-client"
            value={client?.id ?? ""}
            onChange={(event) => setRequestedClient(event.target.value)}
            disabled={!mapped.length}
          >
            {!client ? (
              <option value="">Select or enroll an instance</option>
            ) : null}
            {mapped.map((item) => (
              <option key={item.id} value={item.id}>
                {item.display_name} · {item.status}
              </option>
            ))}
          </Select>
        </div>
      </div>
      {requestedClient && clients.isSuccess && storeId && !client ? (
        <Alert variant="warning">
          The requested runtime client instance is not mapped to this store.
          Select the intended instance before following its registration
          guidance.
        </Alert>
      ) : null}
      <StoreAvailability store={store} />
      <ol className="list-decimal space-y-5 pl-6 text-sm">
        <li className="space-y-2">
          <h3 className="font-semibold">Select an available store</h3>
          <p>
            {available
              ? `${store!.name} is available in the latest backend response.`
              : "Create a store or unlock the selected encrypted store with its existing key."}
          </p>
          <Button asChild size="sm">
            <Link to="/stores" search={{ ...search, create: "store" }}>
              Create store
            </Link>
          </Button>
        </li>
        <li className="space-y-2">
          <h3 className="font-semibold">
            Enroll the frontend's public certificate
          </h3>
          <p>
            Generate the identity on the frontend host with{" "}
            <code>underwrap-keygen</code>. Enroll only its public certificate;
            the private key stays on that host.
          </p>
          {client ? (
            <p>
              {client.display_name} is mapped here and is {client.status}. Its
              backend display name is not the frontend service name.
            </p>
          ) : null}
          {store ? (
            <Button asChild size="sm">
              <Link
                to="/client-instances"
                search={{ ...search, create: "client" }}
              >
                Enroll client instance
              </Link>
            </Button>
          ) : (
            <Button size="sm" disabled>
              Enroll client instance
            </Button>
          )}
        </li>
        <li className="space-y-2" data-testid="setup-registration-path">
          <h3 className="font-semibold">
            Register assets and restart the matching frontend daemon
          </h3>
          {client?.auto_create_wrapper_assets ? (
            <p>
              Asset auto-creation is explicitly enabled for this instance.
              Restart the matching frontend daemon first so it registers its
              configured resources and wrapper/resource pairs, then inspect them
              below.
            </p>
          ) : client ? (
            <p>
              Asset auto-creation is off for this instance. Create the resource
              and wrapper/resource pair manually, matching the root-owned
              frontend configuration, then restart its daemon.
            </p>
          ) : (
            <p>
              Select or enroll a runtime client instance to see its registration
              strategy. New instances have asset auto-creation off unless
              explicitly selected during enrollment.
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            {available ? (
              <>
                <Button asChild size="sm">
                  <Link
                    to="/secrets"
                    search={{ ...search, create: "resource" }}
                  >
                    Create resource
                  </Link>
                </Button>
                <Button asChild size="sm">
                  <Link to="/wrappers" search={{ ...search, create: "pair" }}>
                    Create wrapper/resource pair
                  </Link>
                </Button>
              </>
            ) : (
              <>
                <Button size="sm" disabled>
                  Create resource
                </Button>
                <Button size="sm" disabled>
                  Create wrapper/resource pair
                </Button>
              </>
            )}
          </div>
          <p>
            <strong>Frontend-host step — not observed by this UI:</strong> run{" "}
            <code>systemctl restart underwrapd-&lt;instance&gt;.service</code>{" "}
            for the actual frontend instance. Restart interrupts that daemon's
            traffic and makes it use the enrolled identity.
          </p>
          {view.isSuccess ? (
            <p>
              Backend inventory: {view.data.resources.length} resources and{" "}
              {view.data.pairs.length} registered pairs. These counts do not
              prove frontend configuration or target behavior.
            </p>
          ) : null}
        </li>
        <li className="space-y-2">
          <h3 className="font-semibold">Add values in Secrets</h3>
          <p>
            Create entries in the intended env/files slots or private Secret
            Service tree. Existing values are revealed only on explicit request.
          </p>
          <Link className="underline" to="/secrets" search={search}>
            Open Secrets
          </Link>
        </li>
        <li className="space-y-2">
          <h3 className="font-semibold">
            Configure an explicit individual permission
          </h3>
          <p>
            Choose the exact runtime client instance and wrapper/resource pair,
            then save the intended mode and writable flag. Missing permission
            rows deny access in Individual settings.
          </p>
          {client && client.access_override.kind !== "individual" ? (
            <Alert variant="warning">
              This client has an active client-wide override. Individual rows
              are retained but do not alone determine access. Review its
              override before relying on narrow permissions.
            </Alert>
          ) : null}
          <Link className="underline" to="/wrappers" search={search}>
            Open wrapper permissions
          </Link>
          {client ? (
            <>
              {" "}
              ·{" "}
              <Link
                className="underline"
                to="/client-instances"
                search={{ ...search, client: client.id }}
              >
                Review client override and status
              </Link>
            </>
          ) : null}
        </li>
        <li className="space-y-2">
          <h3 className="font-semibold">
            Verify a representative real wrapper on the frontend
          </h3>
          <p>
            <strong>Operator verification — not observed by this UI:</strong>{" "}
            invoke the intended wrapper and inspect its representative result
            without exposing resource values. Backend metadata and last-seen
            timestamps do not establish that a target works or that writeback
            succeeded.
          </p>
          <a
            className="underline"
            href={`${OPERATIONS_GUIDE}#diagnose-failures-without-exposing-values`}
            target="_blank"
            rel="noreferrer noopener"
          >
            Connection, authorization and writeback diagnosis
          </a>
        </li>
      </ol>
    </div>
  );
}
