import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { fetchLibsecretItemSecret } from "@/api/client";

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
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reveal libsecret item secret</DialogTitle>
          <DialogDescription>
            <code>
              {collectionId} · {itemId}
            </code>{" "}
            in resource <code>{resourceId}</code>.
          </DialogDescription>
        </DialogHeader>
        <RevealSecretView
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
            fetchLibsecretItemSecret(
              storeId,
              resourceId,
              collectionId,
              itemId,
            )
          }
          pickB64={(r) => r.secret_b64}
          textareaId="libsecret-reveal-value"
          textareaTestId="libsecret-reveal-value"
          onClose={onClose}
        />
      </DialogContent>
    </Dialog>
  );
}
