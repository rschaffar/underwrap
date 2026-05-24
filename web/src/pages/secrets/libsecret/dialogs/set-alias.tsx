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
import { Select } from "@/components/ui/select";
import {
  type AdminLibsecretCollectionView,
  putLibsecretAlias,
} from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

import { ID_PATTERN, libsecretTreeKey } from "../internal";

const aliasSchema = z.object({
  name: z
    .string()
    .min(1, "Alias name is required.")
    .refine(
      (s) => ID_PATTERN.test(s),
      "Name must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  collection_id: z.string().min(1, "Pick a collection."),
});
type AliasValues = z.infer<typeof aliasSchema>;

export function SetAliasDialog({
  storeId,
  resourceId,
  collections,
  existingAliases,
  onClose,
}: {
  storeId: string;
  resourceId: string;
  collections: AdminLibsecretCollectionView[];
  existingAliases: string[];
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<AliasValues>({
    schema: aliasSchema,
    defaultValues: {
      name: existingAliases.includes("default") ? "" : "default",
      collection_id: collections[0]?.id ?? "",
    },
    submit: (v) => {
      const values = v as AliasValues;
      return putLibsecretAlias(storeId, resourceId, values.name, {
        collection_id: values.collection_id,
      });
    },
    invalidate: [libsecretTreeKey(storeId, resourceId)],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Set libsecret alias"
      description={
        <>
          Aliases map a well-known name (commonly <code>default</code>)
          to a collection. libsecret clients call <code>ReadAlias</code>{" "}
          to resolve.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="name"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Alias name</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    placeholder="default"
                    {...field}
                  />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="collection_id"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Target collection</FormLabel>
                <FormControl>
                  <Select {...field}>
                    {collections.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.label} ({c.id})
                      </option>
                    ))}
                  </Select>
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
