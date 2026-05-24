import { useState } from "react";
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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { postLibsecretCollection } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof collectionSchema>>({
    resolver: zodResolver(collectionSchema),
    defaultValues: { id: "", label: "" },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof collectionSchema>) =>
      postLibsecretCollection(storeId, resourceId, {
        id: v.id || undefined,
        label: v.label,
      }),
  });
  async function onSubmit(v: z.infer<typeof collectionSchema>) {
    setError(null);
    if (v.id && existingIds.includes(v.id)) {
      form.setError("id", { message: "Id already exists." });
      return;
    }
    try {
      await mut.mutateAsync(v);
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeId, resourceId),
      });
      onClose();
    } catch (e) {
      setError(describeApiError(e));
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create libsecret collection</DialogTitle>
          <DialogDescription>
            A collection holds Secret Service items inside resource{" "}
            <code>{resourceId}</code>.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
            noValidate
          >
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
            {error ? <Alert>{error}</Alert> : null}
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
