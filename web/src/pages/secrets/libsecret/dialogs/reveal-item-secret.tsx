import { api } from "@/api/client";

import { RevealSecretView } from "../../reveal-secret-view";

export function RevealItemSecretDialog({
  storeId,
  resourceId,
  collectionId,
  itemId,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  collectionId: string;
  itemId: string;
  onClose: () => void;
}) {
  return (
    <RevealSecretView
      title="Reveal libsecret item secret"
      description={
        <>
          <code>
            {collectionId} · {itemId}
          </code>{" "}
          in resource <code>{resourceId}</code>.
        </>
      }
      queryKey={[
        "admin",
        "libsecret",
        "item-secret",
        storeId,
        resourceId,
        collectionId,
        itemId,
      ]}
      queryFn={() =>
        api.GET("/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}/secret", { params: { path: { sid: storeId, rid: resourceId, cid: collectionId, iid: itemId } } })
      }
      pickB64={(r) => r.secret_b64}
      textareaId="libsecret-reveal-value"
      textareaTestId="libsecret-reveal-value"
      onClose={onClose}
    />
  );
}
