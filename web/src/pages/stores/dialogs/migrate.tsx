import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import {
  type AdminStoreListItem,
  type AdminStoreMigrateRequest,
  postStoreKeyGenerate,
  postStoreMigrate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { invalidateAllInBackground } from "@/lib/query";

import { KeyConfirmationDialog } from "./key-confirmation";

type StoreBackend = AdminStoreListItem["backend"];

function migrationUnavailableReason(
  store: AdminStoreListItem | null,
  sourceBackend: StoreBackend,
): string | null {
  if (!store) {
    return "The selected store is missing from the current Stores list. Retry is disabled until it reappears in the original source state.";
  }
  if (store.lifecycle_state === "recovery-required") {
    return "Trusted-host recovery is required. Retry is disabled; retain the key because it may protect recoverable data.";
  }
  if (store.backend !== sourceBackend) {
    return "The refreshed Stores list shows that this store already uses the target backend. Retry is disabled; retain the key until you have reconciled the backend outcome or explicitly discard it.";
  }
  if (store.lifecycle_state === "busy") {
    return "A lifecycle operation is busy. Retry is disabled until the source becomes available again.";
  }
  if (store.lifecycle_state === "locked") {
    return "The source is locked. Unlock it before retrying migration.";
  }

  const expectedState = sourceBackend === "plain" ? "ready" : "unlocked";
  if (
    store.lifecycle_state !== expectedState ||
    !store.actions.can_migrate
  ) {
    return "The current source state does not allow migration. Retry is disabled until the Stores list shows the original source backend as available and migratable.";
  }
  return null;
}

export function MigrateStoreDialog({
  storeId,
  currentStore,
  onClose,
}: {
  storeId: string;
  currentStore: AdminStoreListItem | null;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [source] = useState<{ name: string; backend: StoreBackend }>(() => ({
    name: currentStore?.name ?? storeId,
    backend: currentStore?.backend ?? "plain",
  }));
  const [generatedKey, setGeneratedKey] = useState<string | null>(null);
  const [generationPending, setGenerationPending] = useState(false);
  const [mutationPending, setMutationPending] = useState(false);
  const [mutationAttempted, setMutationAttempted] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const toEncrypted = source.backend === "plain";
  const storeName = currentStore?.name ?? source.name;
  const unavailableReason = migrationUnavailableReason(
    currentStore,
    source.backend,
  );
  const canSubmit = unavailableReason === null;

  function refreshStoresInBackground() {
    invalidateAllInBackground(queryClient, [["admin", "stores"]]);
  }

  async function generateKey() {
    if (!canSubmit) return;
    setGenerationPending(true);
    setSubmitError(null);
    try {
      const result = await postStoreKeyGenerate();
      setGeneratedKey(result.key_hex);
    } catch (error) {
      setSubmitError(describeApiError(error));
    } finally {
      setGenerationPending(false);
    }
  }

  async function migrateToEncrypted() {
    if (!generatedKey || !canSubmit) return;
    setMutationAttempted(true);
    setMutationPending(true);
    setSubmitError(null);
    const request: AdminStoreMigrateRequest = {
      backend: "encrypted",
      key_hex: generatedKey,
    };
    try {
      await postStoreMigrate(storeId, request);
      setGeneratedKey(null);
      setMutationAttempted(false);
      refreshStoresInBackground();
      onClose();
    } catch (error) {
      setSubmitError(describeApiError(error));
      refreshStoresInBackground();
    } finally {
      delete request.key_hex;
      setMutationPending(false);
    }
  }

  async function migrateToPlain() {
    if (!canSubmit) return;
    setMutationAttempted(true);
    setMutationPending(true);
    setSubmitError(null);
    try {
      await postStoreMigrate(storeId, { backend: "plain" });
      setMutationAttempted(false);
      refreshStoresInBackground();
      onClose();
    } catch (error) {
      setSubmitError(describeApiError(error));
      refreshStoresInBackground();
    } finally {
      setMutationPending(false);
    }
  }

  function discardKeyAndClose() {
    setGeneratedKey(null);
    setMutationAttempted(false);
    setSubmitError(null);
    onClose();
  }

  if (generatedKey) {
    return (
      <KeyConfirmationDialog
        title={`Confirm migration key for ${storeName}`}
        description="The store was selected for plain-to-encrypted migration. Save and exactly re-enter this fresh backend-generated key before the browser attempts to submit the migration."
        keyHex={generatedKey}
        actionLabel="Migrate to encrypted"
        pendingLabel="Migrating…"
        pending={mutationPending}
        mutationAttempted={mutationAttempted}
        actionDisabledReason={unavailableReason}
        submitError={submitError}
        onConfirm={() => void migrateToEncrypted()}
        onCancel={discardKeyAndClose}
        onDiscard={discardKeyAndClose}
      />
    );
  }

  if (!mutationAttempted && currentStore?.lifecycle_state === "locked") {
    return (
      <AdminDialog
        open
        onClose={onClose}
        title={`Migrate store: ${storeName}`}
        description="This encrypted store must be unlocked before it can be migrated."
      >
        <Alert variant="warning">
          Unlock this store first. Migration needs the current key to read and
          verify the encrypted source. No migration request can be sent while
          the backend action is unavailable.
        </Alert>
        <DialogFooter>
          <Button type="button" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </AdminDialog>
    );
  }

  if (!mutationAttempted && unavailableReason) {
    return (
      <AdminDialog
        open
        onClose={onClose}
        title={`Migrate store: ${storeName}`}
        description="Migration is not currently available for this store."
      >
        <Alert variant="warning">{unavailableReason}</Alert>
        <DialogFooter>
          <Button type="button" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </AdminDialog>
    );
  }

  return (
    <AdminDialog
      open
      nonDismissible={generationPending || mutationPending}
      onClose={onClose}
      title={`${toEncrypted ? "Migrate to encrypted" : "Migrate to plain"}: ${storeName}`}
      description="The store keeps its stable id, metadata, runtime client mappings, control documents, resources, and logical bytes while its storage backend changes."
    >
      <div className="space-y-3">
        <Alert variant="warning">
          Quiesce wrapper traffic and all other admin mutations for this store
          before migration. An invocation opened earlier can otherwise finalize
          later under existing last-writer-wins behavior.
        </Alert>
        <Alert variant="warning">
          {toEncrypted
            ? "Failure or interruption can leave displaced plaintext source data under a private backup workspace on the trusted backend host. Path removal is not secure media erasure."
            : "Failure or interruption can leave plaintext target data under a private stage workspace or at the canonical store path on the trusted backend host. Path removal is not secure media erasure."}
        </Alert>
        {submitError ? <Alert>{submitError}</Alert> : null}
        {mutationAttempted && submitError ? (
          <Alert variant="warning">
            The browser attempted to submit the migration, and it may have
            reached the backend. An error or lost response cannot prove which
            backend is authoritative. Reconcile the Stores list by this
            store&apos;s unchanged id before retrying.
          </Alert>
        ) : null}
        {mutationAttempted && unavailableReason ? (
          <Alert variant="warning">{unavailableReason}</Alert>
        ) : null}
      </div>
      <DialogFooter>
        <Button
          type="button"
          variant="ghost"
          disabled={generationPending || mutationPending}
          onClick={onClose}
        >
          Cancel
        </Button>
        <Button
          type="button"
          variant={toEncrypted ? "default" : "destructive"}
          disabled={
            generationPending || mutationPending || Boolean(unavailableReason)
          }
          onClick={() =>
            void (toEncrypted ? generateKey() : migrateToPlain())
          }
        >
          {generationPending
            ? "Generating…"
            : mutationPending
              ? "Migrating…"
              : toEncrypted
                ? "Generate and review key"
                : mutationAttempted
                  ? "Retry migration to plain"
                  : "Migrate to plain"}
        </Button>
      </DialogFooter>
    </AdminDialog>
  );
}
