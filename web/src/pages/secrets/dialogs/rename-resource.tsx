import { useMemo } from "react";
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
import { putResourceRename } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const renameResourceSchema = z.object({
  new_name: z.string().trim().min(1, "New name is required."),
});
type RenameResourceValues = z.infer<typeof renameResourceSchema>;

/**
 * Rename the operator-facing label of a resource. The backend id and
 * every reference to it (pair records, wrapper permissions, on-disk
 * blob key) stay put — this is a metadata flip.
 */
export function RenameResourceDialog({
  storeId,
  resourceId,
  currentName,
  existingNames,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  currentName: string;
  existingNames: string[];
  onClose: () => void;
}) {
  const otherNames = useMemo(
    () => new Set(existingNames.filter((n) => n !== currentName)),
    [existingNames, currentName],
  );
  const { form, onSubmit, pending, submitError } = useFormDialog<
    RenameResourceValues
  >({
    schema: renameResourceSchema,
    defaultValues: { new_name: currentName },
    submit: (v) => {
      const next = v.new_name.trim();
      if (next === currentName) {
        // No-op rename; resolve without hitting the backend.
        return Promise.resolve(undefined);
      }
      if (otherNames.has(next)) {
        throw new Error("Another resource already uses that name.");
      }
      return putResourceRename(storeId, resourceId, { new_name: next });
    },
    invalidate: [
      ["admin", "secrets-view", storeId],
      ["admin", "wrapper-permissions-view", storeId],
    ],
    onSuccess: onClose,
  });

  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Rename resource"
      description="Change the operator-facing name. The backend id and every pair/permission/secret reference stay put — this is a metadata flip, not a move."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="new_name"
            render={({ field }) => (
              <FormItem>
                <FormLabel>New name</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    data-testid="rename-resource-input"
                    {...field}
                  />
                </FormControl>
                <FormDescription>
                  Current: <code>{currentName}</code>. Wrapper configs
                  referencing the old name will need updating.
                </FormDescription>
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
              {pending ? "Renaming…" : "Rename"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
