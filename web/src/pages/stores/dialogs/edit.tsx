import { z } from "zod";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { type AdminStoreListItem, putStoreUpdate } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const editSchema = z.object({ description: z.string() });
type EditValues = z.infer<typeof editSchema>;

/**
 * Edit a store's description. Renaming and master-key rotation each
 * have their own dedicated dialogs; this one is description-only.
 */
export function EditStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<EditValues>({
    schema: editSchema,
    defaultValues: { description: store.description ?? "" },
    submit: (v) =>
      putStoreUpdate(store.id, {
        name: undefined,
        description: (v as EditValues).description,
      }),
    invalidate: [["admin", "stores"]],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title={`Edit store: ${store.name}`}
      description="Backend type and data directory are immutable. Use the dedicated rotate-key action on encrypted stores."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
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
            <Button type="submit" disabled={pending}>
              {pending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
