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
  postPairCreate,
  postPairWithNewResource,
  type ResourceRecord,
} from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const schema = z
  .object({
    wrapper: z.string().trim().min(1, "Wrapper name is required."),
    mode: z.enum(["existing", "new"]),
    resource_id: z.string().optional(),
    new_resource_name: z.string().trim().optional(),
  })
  .refine((v) => v.mode !== "existing" || !!v.resource_id, {
    path: ["resource_id"],
    message: "Pick an existing resource.",
  })
  .refine((v) => v.mode !== "new" || !!v.new_resource_name, {
    path: ["new_resource_name"],
    message: "Enter a name for the new resource.",
  });
type Values = z.infer<typeof schema>;

export function CreatePairDialog({
  storeId,
  existingResources,
  onClose,
}: {
  storeId: string;
  existingResources: ResourceRecord[];
  onClose: () => void;
}) {
  const hasExisting = existingResources.length > 0;
  const { form, onSubmit, pending, submitError } = useFormDialog<Values>({
    schema,
    defaultValues: {
      wrapper: "",
      mode: hasExisting ? "existing" : "new",
      resource_id: existingResources[0]?.id ?? "",
      new_resource_name: "",
    },
    submit: (v) => {
      const values = v as Values;
      const wrapper = values.wrapper.trim();
      if (values.mode === "new") {
        return postPairWithNewResource(storeId, {
          wrapper,
          resource_name: (values.new_resource_name ?? "").trim(),
        });
      }
      return postPairCreate(storeId, {
        wrapper,
        resource_id: values.resource_id ?? "",
      });
    },
    invalidate: [["admin", "wrapper-permissions-view", storeId]],
    onSuccess: onClose,
  });

  const mode = form.watch("mode");

  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Create wrapper/resource pair"
      description="A wrapper/resource pair is the unit wrapper permissions reference. Each registered pair becomes a row in the table."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-3" noValidate>
          <FormField
            control={form.control}
            name="wrapper"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Wrapper name</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoComplete="off"
                    placeholder="e.g. gh-authorized"
                    {...field}
                  />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="mode"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Resource</FormLabel>
                {hasExisting ? (
                  <div className="flex items-center gap-2 text-sm">
                    <label className="inline-flex items-center gap-1.5">
                      <input
                        type="radio"
                        name="resource-mode"
                        checked={field.value === "existing"}
                        onChange={() => field.onChange("existing")}
                      />
                      Pick existing
                    </label>
                    <label className="inline-flex items-center gap-1.5">
                      <input
                        type="radio"
                        name="resource-mode"
                        checked={field.value === "new"}
                        onChange={() => field.onChange("new")}
                      />
                      Create new
                    </label>
                  </div>
                ) : null}
              </FormItem>
            )}
          />
          {mode === "new" ? (
            <FormField
              control={form.control}
              name="new_resource_name"
              render={({ field }) => (
                <FormItem>
                  <FormControl>
                    <Input
                      data-testid="pair-new-resource"
                      placeholder="e.g. github/token"
                      autoComplete="off"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
          ) : (
            <FormField
              control={form.control}
              name="resource_id"
              render={({ field }) => (
                <FormItem>
                  <FormControl>
                    <Select
                      data-testid="pair-resource-select"
                      className="h-auto px-2 py-1.5"
                      {...field}
                    >
                      {existingResources.map((r) => (
                        <option key={r.id} value={r.id}>
                          {r.name}
                        </option>
                      ))}
                    </Select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
          )}
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
