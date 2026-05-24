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
  const { form, onSubmit, pending, submitError } = useFormDialog<ResourceValues>({
    schema: resourceSchema,
    defaultValues: { resource: "" },
    submit: (v) => {
      const name = (v as ResourceValues).resource;
      if (known.has(name)) {
        throw new Error("A resource with that name already exists.");
      }
      return postResourceCreate(storeId, { name });
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
