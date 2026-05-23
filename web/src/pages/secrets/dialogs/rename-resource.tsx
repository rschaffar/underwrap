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
import { putResourceRename } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  storeName,
  resourceId,
  currentName,
  existingNames,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  currentName: string;
  existingNames: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const form = useForm<RenameResourceValues>({
    resolver: zodResolver(renameResourceSchema),
    defaultValues: { new_name: currentName },
  });
  const otherNames = useMemo(
    () => new Set(existingNames.filter((n) => n !== currentName)),
    [existingNames, currentName],
  );
  const mut = useMutation({
    mutationFn: (new_name: string) =>
      putResourceRename(storeName, resourceId, { new_name }),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ["admin", "secrets-view", storeName],
        }),
        qc.invalidateQueries({
          queryKey: ["admin", "wrapper-permissions-view", storeName],
        }),
      ]);
      onClose();
    },
    onError: (err) => {
      form.setError("new_name", { message: describeApiError(err) });
    },
  });

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rename resource</DialogTitle>
          <DialogDescription>
            Change the operator-facing name. The backend id and every
            pair/permission/secret reference stay put — this is a
            metadata flip, not a move.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              const next = v.new_name.trim();
              if (next === currentName) {
                onClose();
                return;
              }
              if (otherNames.has(next)) {
                form.setError("new_name", {
                  message: "Another resource already uses that name.",
                });
                return;
              }
              mut.mutate(next);
            })}
            className="space-y-4"
            noValidate
          >
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
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Renaming…" : "Rename"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
