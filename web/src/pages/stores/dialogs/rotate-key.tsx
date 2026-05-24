import { useState } from "react";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { useConfirmMutation } from "@/hooks/use-confirm-mutation";
import { type AdminStoreListItem, postStoreRotateKey } from "@/api/client";

import { KeyRevealDialog } from "./key-reveal";

/**
 * Rotate an encrypted store's master key. The backend mints a fresh
 * 32-byte key, re-encrypts every resource under it, and returns the
 * new key once — surfaced to the operator via the [`KeyRevealDialog`]
 * follow-up that requires explicit acknowledgement before close.
 */
export function RotateStoreKeyDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const [ack, setAck] = useState(false);
  const [revealKey, setRevealKey] = useState<{
    keyHex: string;
    warning: string;
  } | null>(null);

  const { confirm, pending, submitError } = useConfirmMutation({
    mutationFn: () => postStoreRotateKey(store.id),
    invalidate: [["admin", "stores"]],
    onSuccess: () => {
      // The result is captured via the override below.
    },
  });

  async function handleRotate() {
    const result = (await confirm()) as { key_hex: string; warning: string };
    setRevealKey({ keyHex: result.key_hex, warning: result.warning });
  }

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`New encryption key for ${store.name}`}
        description={
          <>
            <strong>Heads up:</strong> {revealKey.warning} This is the only time
            the new key will be shown.
          </>
        }
        keyHex={revealKey.keyHex}
        onClose={() => {
          setRevealKey(null);
          onClose();
        }}
      />
    );
  }

  return (
    <AdminDialog
      open
      onClose={onClose}
      title={`Rotate master key: ${store.name}`}
      description="Mints a fresh 32-byte master key and re-encrypts every resource in this store under it. Writes are briefly blocked while the data directory is swapped; reads continue throughout. The new key is shown once after the rotation completes — save it before closing the dialog."
    >
      {store.locked ? (
        <Alert variant="warning">
          The store is locked. Unlock it first; rotation needs the current key
          to read existing ciphertext.
        </Alert>
      ) : null}
      <label className="flex items-start gap-2 text-xs text-(--color-muted)">
        <input
          type="checkbox"
          disabled={store.locked}
          checked={ack}
          onChange={(e) => setAck(e.target.checked)}
        />
        <span>
          I understand every resource will be re-encrypted and a new master key
          will be issued.
        </span>
      </label>
      {submitError ? <Alert>{submitError}</Alert> : null}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button
          type="button"
          variant="destructive"
          disabled={store.locked || !ack || pending}
          onClick={() => void handleRotate()}
        >
          {pending ? "Rotating…" : "Rotate key"}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
