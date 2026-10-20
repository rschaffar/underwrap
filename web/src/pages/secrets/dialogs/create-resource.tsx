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
import { postResourceCreate } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const resourceSchema = z.object({
  resource: z.string().trim().min(1, "Resource name is required."),
});
type ResourceValues = z.infer<typeof resourceSchema>;

export function CreateResourceDialog({
  storeId,
  existingResources,
  onClose,
}: {
  storeId: string;
  existingResources: string[];
  onClose: () => void;
}) {
  const known = useMemo(() => new Set(existingResources), [existingResources]);
  // Duplicate detection is a field-level validation so the operator sees
  // an inline message under the input, not a generic submit-error Alert.
  const schema = useMemo(
    () =>
      resourceSchema.refine((v) => !known.has(v.resource), {
        path: ["resource"],
        message: "A resource with that name already exists.",
      }),
    [known],
  );
  const { form, onSubmit, pending, submitError } = useFormDialog<ResourceValues>({
    schema,
    defaultValues: { resource: "" },
    submit: (v) => postResourceCreate(storeId, { name: v.resource }),
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
      nonDismissible={pending}
      title="Create resource"
      description={
        <>
          Add a resource record to this store. Use the{" "}
          <strong className="text-(--color-fg)">+</strong> button on each
          slot (env, files, libsecret) to populate it.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="resource"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Resource name</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    placeholder="e.g. github/token"
                    data-testid="create-resource-input"
                    {...field}
                  />
                </FormControl>
                <FormDescription>
                  A human-readable label. The on-disk id is generated.
                </FormDescription>
                <FormMessage />
              </FormItem>
            )}
          />
          {submitError ? <Alert>{submitError}</Alert> : null}
          <DialogFooter>
            <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
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
