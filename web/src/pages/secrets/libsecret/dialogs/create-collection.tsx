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
import { postLibsecretCollection } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

import { ID_PATTERN, libsecretTreeKey } from "../internal";

const collectionSchema = z.object({
  id: z
    .string()
    .optional()
    .refine(
      (s) => !s || ID_PATTERN.test(s),
      "Id must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  label: z.string().min(1, "Label is required."),
});
type CollectionValues = z.infer<typeof collectionSchema>;

export function CreateCollectionDialog({
  storeId,
  resourceId,
  existingIds,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  existingIds: string[];
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<CollectionValues>({
    schema: collectionSchema,
    defaultValues: { id: "", label: "" },
    submit: (v) => {
      const values = v as CollectionValues;
      if (values.id && existingIds.includes(values.id)) {
        throw new Error("Id already exists.");
      }
      return postLibsecretCollection(storeId, resourceId, {
        id: values.id || undefined,
        label: values.label,
      });
    },
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Create libsecret collection"
      description={
        <>
          A collection holds Secret Service items inside resource{" "}
          <code>{resourceId}</code>.
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
                    placeholder="auto-generated if blank, e.g. default"
                    {...field}
                  />
                </FormControl>
                <FormDescription>
                  Stable identifier. Libsecret clients reach this
                  collection via <code>ReadAlias</code> or by id.
                </FormDescription>
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
                    placeholder="e.g. Default"
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
