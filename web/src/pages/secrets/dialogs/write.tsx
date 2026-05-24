import { z } from "zod";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
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
import { useFormDialog } from "@/hooks/use-form-dialog";

import type { SecretAddress } from "../model";

// The dialog edits the structured `(resource, slot, key)` address
// directly. Resource and slot are locked in both modes — they're
// pinned by the page row that opened the dialog, and changing them
// would amount to moving an entry across resources/slots, which the
// API doesn't model as a single operation. Key is editable in both
// modes; on edit, a key change is implemented via the atomic
// load-blob → swap-entry → save rename endpoint so the operator never
// sees two rows during the swap.

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
  const { form, onSubmit, pending, submitError } = useFormDialog<WriteValues>({
    schema: writeSchema,
    defaultValues: { key: address.key, material: "" },
    submit: (v) => {
      const { key, material } = v as WriteValues;
      if (mode === "edit" && key !== address.key) {
        return postSecretRename(
          storeId,
          address.resource_id,
          address.slot,
          address.key,
          { to_key: key, material_b64: btoa(material) },
        );
      }
      return putSecret(storeId, address.resource_id, address.slot, key, {
        material_b64: btoa(material),
      });
    },
    invalidate: [["admin", "secrets-view", storeId]],
    onSuccess: onClose,
  });

  return (
    <AdminDialog
      open
      onClose={onClose}
      title={mode === "create" ? `Create ${address.slot} entry` : `Edit ${address.slot} entry`}
      description={
        <>
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
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
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
              {pending ? "Saving…" : mode === "create" ? "Create" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
