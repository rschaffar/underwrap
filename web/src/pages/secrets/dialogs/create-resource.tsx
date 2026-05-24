import { useMemo } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { postResourceCreate } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  const qc = useQueryClient();
  const form = useForm<ResourceValues>({
    resolver: zodResolver(resourceSchema),
    defaultValues: { resource: "" },
  });
  const known = useMemo(() => new Set(existingResources), [existingResources]);
  const mut = useMutation({
    mutationFn: (name: string) =>
      postResourceCreate(storeId, { name }),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ["admin", "secrets-view", storeId],
        }),
        qc.invalidateQueries({
          queryKey: ["admin", "wrapper-permissions-view", storeId],
        }),
      ]);
      onClose();
    },
    onError: (err) => {
      form.setError("resource", { message: describeApiError(err) });
    },
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create resource</DialogTitle>
          <DialogDescription>
            Add a resource record to this store. Use the{" "}
            <strong className="text-(--color-fg)">+</strong> button on each
            slot (env, files, libsecret) to populate it.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              if (known.has(v.resource)) {
                form.setError("resource", {
                  message: "A resource with that name already exists.",
                });
                return;
              }
              mut.mutate(v.resource);
            })}
            className="space-y-4"
            noValidate
          >
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
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
