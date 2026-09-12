import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import {
  ApiError,
  type AdminStoreListItem,
  postStoreRotateKey,
} from "@/api/client";
import { ROTATION_RECOVERY_GUIDE } from "@/lib/operator-guidance";
import { describeApiError } from "@/lib/error-message";
import { invalidateAllInBackground } from "@/lib/query";

import { KeyRevealDialog } from "./key-reveal";

// These named responses occur before generation-changing work is admitted.
// A generic status/transport failure does not establish that no rotation ran.
const PRE_EFFECT_REJECTIONS = new Set([
  "not_found",
  "not_encrypted",
  "store_locked",
  "store_busy",
  "migration_recovery_required",
  "csrf_required",
  "csrf_mismatch",
  "session_revoked",
]);

/**
 * Rotate an encrypted store's master key. The backend mints a fresh
 * 32-byte key, re-encrypts every resource under it, and returns the
 * new key once — surfaced to the operator via the [`KeyRevealDialog`]
 * follow-up that requires explicit acknowledgement before close.
 */
export function RotateStoreKeyDialog({
  storeId,
  currentStore,
  onClose,
}: {
  storeId: string;
  currentStore: AdminStoreListItem | null;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [initialName] = useState(() => currentStore?.name ?? storeId);
  const [ack, setAck] = useState(false);
  const [pending, setPending] = useState(false);
  const inFlight = useRef(false);
  const [outcomeUnknown, setOutcomeUnknown] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [revealKey, setRevealKey] = useState<{
    keyHex: string;
    warning: string;
  } | null>(null);

  const storeName = currentStore?.name ?? initialName;
  const locked = currentStore?.lifecycle_state === "locked";
  const canRotate =
    currentStore?.lifecycle_state === "unlocked" &&
    currentStore.actions.can_rotate_key;

  async function handleRotate() {
    if (!canRotate || !ack || inFlight.current || outcomeUnknown) return;
    inFlight.current = true;
    setPending(true);
    setSubmitError(null);
    try {
      const result = await postStoreRotateKey(storeId);
      if (
        !result ||
        typeof result.key_hex !== "string" ||
        !/^[0-9a-fA-F]{64}$/.test(result.key_hex) ||
        typeof result.warning !== "string"
      ) {
        throw new Error("Rotation response was incomplete.");
      }
      setRevealKey({ keyHex: result.key_hex, warning: result.warning });
      invalidateAllInBackground(queryClient, [["admin", "stores"]]);
    } catch (error) {
      const rejectedBeforeEffects =
        error instanceof ApiError &&
        (error.status === 401 ||
          error.status === 403 ||
          PRE_EFFECT_REJECTIONS.has(error.code));
      setOutcomeUnknown(!rejectedBeforeEffects);
      setAck(false);
      setSubmitError(
        rejectedBeforeEffects
          ? describeApiError(error)
          : "Rotation outcome unknown. The replacement key may not have reached this browser. Do not rotate again until a trusted backend-host operator has reconciled the generations and key custody.",
      );
      invalidateAllInBackground(queryClient, [["admin", "stores"]]);
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  }

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`New encryption key for ${storeName}`}
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
      onClose={() => {
        if (!inFlight.current) onClose();
      }}
      nonDismissible={pending}
      title={`Rotate master key: ${storeName}`}
      description="Quiesce wrapper traffic and other admin access to this store first: rotation is serialized only against another rotation, so concurrent reads may fail and concurrent writes may be lost. The operation mints a fresh 32-byte key, re-encrypts the store, and shows the new key once. Every displaced ciphertext generation remains on the backend host, consumes disk, and stays decryptable with its old key until a trusted host operator removes that selected generation after independently verifying a later key."
    >
      {locked ? (
        <Alert variant="warning">
          The store is locked. Unlock it first; rotation needs the current key
          to read existing ciphertext.
        </Alert>
      ) : null}
      {!currentStore || (!locked && !canRotate) ? (
        <Alert variant="warning">
          Rotation is not currently available for this store. Reconcile the
          current Stores list before trying again.
        </Alert>
      ) : null}
      <label className="flex items-start gap-2 text-xs text-(--color-muted)">
        <input
          type="checkbox"
          disabled={!canRotate || pending || outcomeUnknown}
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
      {outcomeUnknown ? (
        <p className="text-sm text-(--color-muted)">
          The Stores list cannot prove key delivery. Preserve existing keys and
          retained generations.{" "}
          <a
            className="underline"
            href={ROTATION_RECOVERY_GUIDE}
            target="_blank"
            rel="noreferrer noopener"
          >
            Trusted-host rotation recovery
          </a>
          .
        </p>
      ) : null}
      <DialogFooter>
        <Button
          type="button"
          variant="ghost"
          disabled={pending}
          onClick={() => {
            if (!inFlight.current) onClose();
          }}
        >
          {outcomeUnknown ? "Close" : "Cancel"}
        </Button>
        <Button
          type="button"
          variant="destructive"
          disabled={!canRotate || !ack || pending || outcomeUnknown}
          onClick={() => void handleRotate()}
        >
          {pending ? "Rotating…" : "Rotate key"}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
