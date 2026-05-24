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

import { useFormDialog } from "@/hooks/use-form-dialog";

import { ID_PATTERN, libsecretTreeKey, parseAttributes } from "../internal";
import { postLibsecretItem } from "@/api/client";

const itemSchema = z.object({
  id: z
    .string()
    .optional()
    .refine(
      (s) => !s || ID_PATTERN.test(s),
      "Id must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  label: z.string().min(1, "Label is required."),
  content_type: z.string().min(1, "Content type is required."),
  attributes: z.string(),
  secret: z.string().min(1, "Secret material is required."),
});
type ItemValues = z.infer<typeof itemSchema>;

export function CreateItemDialog({
  storeId,
  resourceId,
  collectionId,
  existingIds,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  collectionId: string;
  existingIds: string[];
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<ItemValues>({
    schema: itemSchema,
    defaultValues: {
      id: "",
      label: "",
      content_type: "text/plain",
      attributes: "",
      secret: "",
    },
    submit: (v) => {
      const values = v as ItemValues;
      if (values.id && existingIds.includes(values.id)) {
        throw new Error("Id already exists in this collection.");
      }
      return postLibsecretItem(storeId, resourceId, collectionId, {
        id: values.id || undefined,
        label: values.label,
        content_type: values.content_type,
        attributes: parseAttributes(values.attributes),
        secret_b64: btoa(values.secret),
      });
    },
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Create libsecret item"
      description={
        <>
          In collection <code>{collectionId}</code> of resource{" "}
          <code>{resourceId}</code>. Secret is sent over TLS,
          base64-encoded in transit, and never echoed back.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="id"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Id (optional)</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    placeholder="auto-generated if blank, e.g. item_1"
                    {...field}
                  />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="label"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Label</FormLabel>
                <FormControl>
                  <Input
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    placeholder="e.g. GitHub Token"
                    {...field}
                  />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="content_type"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Content type</FormLabel>
                <FormControl>
                  <Input
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    placeholder="text/plain"
                    {...field}
                  />
                </FormControl>
                <FormDescription>
                  The MIME-style type libsecret carries in the Secret
                  struct. Most apps use <code>text/plain</code>.
                </FormDescription>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="attributes"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Attributes (optional)</FormLabel>
                <FormControl>
                  <textarea
                    data-testid="libsecret-item-attributes-input"
                    rows={3}
                    className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                    placeholder={
                      "xdg:schema=org.freedesktop.Secret.Generic\nservice=github.com"
                    }
                    {...field}
                  />
                </FormControl>
                <FormDescription>
                  One <code>key=value</code> per line. libsecret looks
                  these up case-sensitively.
                </FormDescription>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="secret"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Secret material</FormLabel>
                <FormControl>
                  <textarea
                    data-testid="libsecret-item-secret-input"
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
              {pending ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
