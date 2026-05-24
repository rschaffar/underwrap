import { api } from "@/api/client";

import type { SecretAddress } from "../grouping";
import { RevealSecretView } from "../reveal-secret-view";

export function RevealSecretDialog({
  storeId,
  address,
  onClose,
}: {
  storeId: string;
  address: SecretAddress;
  onClose: () => void;
}) {
  return (
    <RevealSecretView
      title="Reveal secret"
      description={
        <>
          <code>
            {address.resource_id} · {address.slot} · {address.key}
          </code>{" "}
          from <code>{storeId}</code>.
        </>
      }
      queryKey={[
        "admin",
        "secret",
        "read",
        storeId,
        address.resource_id,
        address.slot,
        address.key,
      ]}
      queryFn={() =>
        api.GET("/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}", { params: { path: { id: storeId, rid: address.resource_id, slot: address.slot, key: address.key } } })
      }
      pickB64={(r) => r.material_b64}
      textareaId="reveal-value"
      textareaTestId="reveal-value"
      onClose={onClose}
    />
  );
}
