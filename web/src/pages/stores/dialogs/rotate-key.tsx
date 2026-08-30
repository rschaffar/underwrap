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
  });

  async function handleRotate() {
    const outcome = await confirm();
    if (outcome.status === "error") return;
    const result = outcome.result;
    setRevealKey({ keyHex: result.key_hex, warning: result.warning });
  }

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`New encryption key for ${store.name}`}
        description={
          <>
            <strong>Heads up:</strong> {revealKey.warning} This is the only time
            the new key will be shown. The retained old ciphertext consumes
            backend-host disk and remains decryptable with the old key until a
            trusted host operator removes only the selected generation after
            replacement-key verification.
          </>
        }
        keyHex={revealKey.keyHex}
        acknowledgement={
          <>
            I understand closing this dialog discards the one-time local
            display. The backend cannot know whether I received, saved, or
            independently verified this replacement key.
          </>
        }
        guidance={
          <>
            The retained old ciphertext consumes backend-host disk and remains
            decryptable with the old key. A trusted host operator removes only
            the selected generation after replacement-key verification.
          </>
        }
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
      description="Quiesce wrapper traffic and other admin access to this store first: rotation is serialized only against another rotation, so concurrent reads may fail and concurrent writes may be lost. The operation mints a fresh 32-byte key, re-encrypts the store, and shows the new key once. Every displaced ciphertext generation remains on the backend host, consumes disk, and stays decryptable with its old key until a trusted host operator removes that selected generation after independently verifying a later key."
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
          I understand every resource will be re-encrypted, a new master key
          will be issued once, and old ciphertext requires separate trusted-host
          verification and manual cleanup.
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
