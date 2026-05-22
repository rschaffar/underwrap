import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  type AdminStoreDetailResponse,
  postStoreRotateKey,
  putStoreUpdate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

import { KeyRevealDialog } from "./key-reveal";

const editSchema = z.object({ description: z.string() });
type EditValues = z.infer<typeof editSchema>;

export function EditStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [rotateConfirm, setRotateConfirm] = useState(false);
  const [rotateWarning, setRotateWarning] = useState<string | null>(null);
  const [revealKey, setRevealKey] = useState<{
    storeName: string;
    keyHex: string;
    warning: string;
  } | null>(null);
  const form = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: { description: store.description ?? "" },
  });
  const mut = useMutation({
    mutationFn: (description: string) =>
      putStoreUpdate(store.id, { name: undefined, description }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const rotate = useMutation({
    mutationFn: () => postStoreRotateKey(store.id),
    onSuccess: (res) => {
      setRotateWarning(null);
      setRevealKey({
        storeName: store.name,
        keyHex: res.key_hex,
        warning: res.warning,
      });
    },
    onError: (err) => setRotateWarning(describeApiError(err)),
  });

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`New encryption key for ${revealKey.storeName}`}
        description={
          <>
            <strong>Heads up:</strong> {revealKey.warning} This is the
            only time the new key will be shown.
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

  const canRotate = store.actions.can_rotate_key;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit store: {store.name}</DialogTitle>
          <DialogDescription>
            Backend type and data directory are immutable. Only the
            description can be edited here. Encrypted stores also
            support master-key rotation.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v.description);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Description</FormLabel>
                  <FormControl>
                    <Input autoFocus {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {submitError ? <Alert>{submitError}</Alert> : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
        {store.backend === "encrypted" ? (
          <div className="mt-6 space-y-3 rounded-md border border-(--color-line) bg-(--color-surface-1) p-4">
            <h3 className="text-sm font-semibold text-(--color-fg)">
              Rotate master key
            </h3>
            <p className="text-xs text-(--color-muted)">
              Mints a fresh 32-byte master key and re-encrypts every
              resource in this store under it. Writes are briefly
              blocked while the data directory is swapped; reads
              continue throughout. The new key is shown once after the
              rotation completes — save it before closing the dialog.
            </p>
            {store.locked ? (
              <Alert variant="warning">
                The store is locked. Unlock it first; rotation needs
                the current key to read existing ciphertext.
              </Alert>
            ) : null}
            <label className="flex items-start gap-2 text-xs text-(--color-muted)">
              <input
                type="checkbox"
                disabled={store.locked}
                checked={rotateConfirm}
                onChange={(e) => setRotateConfirm(e.target.checked)}
              />
              <span>
                I understand every resource will be re-encrypted and a
                new master key will be issued.
              </span>
            </label>
            {rotateWarning ? <Alert>{rotateWarning}</Alert> : null}
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={!canRotate || store.locked || !rotateConfirm || rotate.isPending}
              title={
                store.locked
                  ? "Unlock the store first."
                  : !canRotate
                    ? "This store does not support key rotation."
                    : undefined
              }
              onClick={() => rotate.mutate()}
            >
              {rotate.isPending ? "Rotating…" : "Rotate key"}
            </Button>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
