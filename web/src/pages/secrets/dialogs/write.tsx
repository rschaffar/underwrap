import { z } from "zod";

import { postSecretRename, putSecret } from "@/api/client";
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
import { useFormDialog } from "@/hooks/use-form-dialog";
import { encodeUtf8Base64 } from "@/lib/utf8-base64";

import type { SecretAddress } from "../grouping";

const writeFields = z.object({
  key: z.string().min(1, "Key is required."),
  material: z.string(),
  storeEmpty: z.boolean(),
});
type WriteValues = z.infer<typeof writeFields>;

export function WriteSecretDialog({
  storeId,
  mode,
  address,
  resourceName,
  existingKeys,
  onClose,
}: {
  storeId: string;
  mode: "create" | "replace" | "rename";
  /** Resource and slot stay fixed; create opens with an empty key. */
  address: SecretAddress;
  resourceName: string;
  /** Known keys in this resource and slot, not keys from other slots. */
  existingKeys: readonly string[];
  onClose: () => void;
}) {
  const schema = writeFields.superRefine((values, ctx) => {
    if (
      mode !== "replace" &&
      existingKeys.includes(values.key) &&
      !(mode === "rename" && values.key === address.key)
    ) {
      ctx.addIssue({
        code: "custom",
        path: ["key"],
        message:
          mode === "create"
            ? "This key already exists. Use Replace value to change its value."
            : "This key already exists. Choose a different key.",
      });
    }
    if (mode === "rename" && values.key === address.key) {
      ctx.addIssue({
        code: "custom",
        path: ["key"],
        message: "Choose a different key to rename this entry.",
      });
    }
    if (
      mode !== "rename" &&
      !values.storeEmpty &&
      values.material.length === 0
    ) {
      ctx.addIssue({
        code: "custom",
        path: ["material"],
        message: "Supply a new value or select Store an empty value.",
      });
    }
  });

  const { form, onSubmit, pending, submitError } = useFormDialog<WriteValues>({
    schema,
    defaultValues: { key: address.key, material: "", storeEmpty: false },
    submit: ({ key, material, storeEmpty }) => {
      if (mode === "rename") {
        // Omitting material preserves the backend's existing value.
        return postSecretRename(
          storeId,
          address.resource_id,
          address.slot,
          address.key,
          { to_key: key },
        );
      }
      return putSecret(
        storeId,
        address.resource_id,
        address.slot,
        mode === "replace" ? address.key : key,
        { material_b64: storeEmpty ? "" : encodeUtf8Base64(material) },
      );
    },
    invalidate: [["admin", "secrets-view", storeId]],
    onSuccess: onClose,
  });

  const busy = pending;
  const close = () => {
    if (!pending) onClose();
  };
  const unchanged = mode === "rename" && form.watch("key") === address.key;
  const storeEmpty = form.watch("storeEmpty");
  const action =
    mode === "create"
      ? `Create ${address.slot} entry`
      : mode === "replace"
        ? "Replace value"
        : `Rename ${address.slot} entry`;

  return (
    <AdminDialog
      open
      onClose={close}
      nonDismissible={busy}
      title={`${action} — ${resourceName}`}
      description={
        <>
          Resource ID: <code>{address.resource_id}</code>. Slot:{" "}
          <code>{address.slot}</code>. Resource and slot stay unchanged.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <p className="text-sm text-(--color-muted)">
            {mode === "rename"
              ? "Keep existing value. Only the key changes; this dialog does not fetch or display the value."
              : mode === "replace"
                ? "Supply a replacement value. The existing value is not fetched or displayed."
                : "Supply a value for a new key. To change an existing value, use Replace value."}
          </p>
          <FormField
            control={form.control}
            name="key"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Key</FormLabel>
                <FormControl>
                  <Input
                    {...field}
                    autoFocus={mode !== "replace"}
                    autoComplete="off"
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    readOnly={mode === "replace"}
                    disabled={busy}
                    placeholder={
                      address.slot === "env"
                        ? "e.g. API_TOKEN"
                        : "e.g. config.yaml"
                    }
                  />
                </FormControl>
                <FormDescription>
                  {mode === "replace"
                    ? "The key stays unchanged. Use Rename to change it."
                    : address.slot === "env"
                      ? "Environment variable name — letters, digits, and underscores."
                      : "Relative path inside the slot."}
                </FormDescription>
                <FormMessage />
              </FormItem>
            )}
          />
          {mode !== "rename" ? (
            <>
              <FormField
                control={form.control}
                name="material"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>New value</FormLabel>
                    <FormControl>
                      <textarea
                        {...field}
                        data-testid="material-input"
                        autoFocus={mode === "replace"}
                        autoComplete="off"
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        disabled={busy || storeEmpty}
                        rows={4}
                        className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      />
                    </FormControl>
                    <FormDescription>
                      Sent over TLS, base64-encoded in transit. A blank field
                      alone does not store an empty value.
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="storeEmpty"
                render={({ field }) => (
                  <FormItem>
                    <div className="flex items-center gap-2">
                      <FormControl>
                        <input
                          type="checkbox"
                          name={field.name}
                          ref={field.ref}
                          onBlur={field.onBlur}
                          checked={field.value}
                          disabled={busy}
                          onChange={(event) => {
                            field.onChange(event.target.checked);
                            if (event.target.checked) {
                              form.setValue("material", "", {
                                shouldValidate: true,
                              });
                            }
                          }}
                        />
                      </FormControl>
                      <FormLabel>Store an empty value</FormLabel>
                    </div>
                    <FormDescription>
                      {mode === "replace"
                        ? "Explicitly replace the existing value with zero bytes."
                        : "Explicitly create this entry with zero bytes."}
                    </FormDescription>
                  </FormItem>
                )}
              />
            </>
          ) : null}
          <p className="text-xs text-(--color-muted)">
            Coordinate other writers before saving. Concurrent resource changes
            are last-writer-wins.
          </p>
          {submitError ? <Alert>{submitError}</Alert> : null}
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={close}
              disabled={busy}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={busy || unchanged}>
              {busy ? "Saving…" : mode === "create" ? "Create" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
