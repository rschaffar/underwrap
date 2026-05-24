import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { fetchSecret } from "@/api/client";

import type { SecretAddress } from "../model";
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
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reveal secret</DialogTitle>
          <DialogDescription>
            <code>
              {address.resource_id} · {address.slot} · {address.key}
            </code>{" "}
            from <code>{storeId}</code>.
          </DialogDescription>
        </DialogHeader>
        <RevealSecretView
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
            fetchSecret(
              storeId,
              address.resource_id,
              address.slot,
              address.key,
            )
          }
          pickB64={(r) => r.material_b64}
          textareaId="reveal-value"
          textareaTestId="reveal-value"
          onClose={onClose}
        />
      </DialogContent>
    </Dialog>
  );
}
