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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { postSecretRename, putSecret } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

import type { SecretAddress } from "../model";

// The dialog edits the structured `(resource, slot, key)` address
// directly. Resource and slot are locked in both modes — they're
// pinned by the page row that opened the dialog, and changing them
// would amount to moving an entry across resources/slots, which the
// API doesn't model as a single operation. Key is editable in both
// modes; on edit, a key change is implemented as write-new +
// delete-old (best-effort: a failure between the two leaves both
// entries present until the operator deletes one manually).

const writeSchema = z.object({
  key: z.string().min(1, "Key is required."),
  material: z.string().min(1, "Material is required."),
});
type WriteValues = z.infer<typeof writeSchema>;

export function WriteSecretDialog({
  storeId,
  mode,
  address,
  onClose,
}: {
  storeId: string;
  mode: "create" | "edit";
  /**
   * The address the dialog opens against. In `create` mode `key` is
   * the empty string; in `edit` mode it's the existing key.
   */
  address: SecretAddress;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<WriteValues>({
    resolver: zodResolver(writeSchema),
    defaultValues: { key: address.key, material: "" },
  });

  const writeMut = useMutation({
    mutationFn: (v: { key: string; material: string }) =>
      putSecret(storeId, address.resource_id, address.slot, v.key, {
        material_b64: btoa(v.material),
      }),
  });
  // The atomic rename endpoint does load-blob → swap-entry → save in
  // one handler, so a key change can never leave the operator with two
  // rows visible. Used only on the edit-with-key-change path.
  const renameMut = useMutation({
    mutationFn: (v: { from_key: string; to_key: string; material?: string }) =>
      postSecretRename(
        storeId,
        address.resource_id,
        address.slot,
        v.from_key,
        {
          to_key: v.to_key,
          material_b64:
            v.material === undefined ? undefined : btoa(v.material),
        },
      ),
  });

  async function onSubmit(v: WriteValues) {
    setSubmitError(null);
    try {
      if (mode === "edit" && v.key !== address.key) {
        // Atomic rename. Material is always supplied on submit (the
        // textarea is required), so we send it along; the backend
        // overwrites in the same step.
        await renameMut.mutateAsync({
          from_key: address.key,
          to_key: v.key,
          material: v.material,
        });
      } else {
        await writeMut.mutateAsync(v);
      }
      await qc.invalidateQueries({
        queryKey: ["admin", "secrets-view", storeId],
      });
      onClose();
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const pending = writeMut.isPending || renameMut.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {mode === "create"
              ? `Create ${address.slot} entry`
              : `Edit ${address.slot} entry`}
          </DialogTitle>
          <DialogDescription>
            In resource{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
              {address.resource_id}
            </code>
            , slot{" "}
            <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
              {address.slot}
            </code>
            . Material is sent over TLS, base64-encoded in transit, and
            never echoed back.
          </DialogDescription>
        </DialogHeader>

        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="key"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Key</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder={
                        address.slot === "env"
                          ? "e.g. GH_TOKEN"
                          : address.slot === "files"
                            ? "e.g. config.yaml or nested/dir/file"
                            : "e.g. default/gh-cli.secret"
                      }
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    {address.slot === "env"
                      ? "Environment variable name — letters, digits, and underscores."
                      : "Relative path inside the slot. Renaming this key on edit moves the entry."}
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="material"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Material</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="material-input"
                      rows={4}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      {...field}
                    />
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
              <Button type="submit" disabled={pending}>
                {pending
                  ? "Saving…"
                  : mode === "create"
                    ? "Create"
                    : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
