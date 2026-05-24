import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { describeApiError } from "@/lib/error-message";

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
  const qc = useQueryClient();
  const hasExisting = existingResources.length > 0;
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      wrapper: "",
      mode: hasExisting ? "existing" : "new",
      resource_id: existingResources[0]?.id ?? "",
      new_resource_name: "",
    },
  });

  const createPair = useMutation({
    mutationFn: (vars: { wrapper: string; resource_id: string }) =>
      postPairCreate(storeId, {
        wrapper: vars.wrapper,
        resource_id: vars.resource_id,
      }),
  });
  const createPairWithNewResource = useMutation({
    mutationFn: (vars: { wrapper: string; resource_name: string }) =>
      postPairWithNewResource(storeId, vars),
  });
  const pending = createPair.isPending || createPairWithNewResource.isPending;

  async function onSubmit(values: Values) {
    form.clearErrors("root");
    const w = values.wrapper.trim();
    try {
      if (values.mode === "new") {
        await createPairWithNewResource.mutateAsync({
          wrapper: w,
          resource_name: (values.new_resource_name ?? "").trim(),
        });
      } else {
        await createPair.mutateAsync({
          wrapper: w,
          resource_id: values.resource_id ?? "",
        });
      }
      await qc.invalidateQueries({
        queryKey: ["admin", "wrapper-permissions-view", storeId],
      });
      onClose();
    } catch (e) {
      form.setError("root", { message: describeApiError(e) });
    }
  }

  const mode = form.watch("mode");

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create wrapper/resource pair</DialogTitle>
          <DialogDescription>
            A wrapper/resource pair is the unit wrapper permissions
            reference. Each registered pair becomes a row in the table.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-3"
            noValidate
          >
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
            {form.formState.errors.root ? (
              <Alert>{form.formState.errors.root.message}</Alert>
            ) : null}
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
      </DialogContent>
    </Dialog>
  );
}
