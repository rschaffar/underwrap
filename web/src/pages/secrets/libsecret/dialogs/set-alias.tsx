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
import { describeApiError } from "@/lib/error-message";

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

export function SetAliasDialog({
  storeName,
  resourceId,
  collections,
  existingAliases,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collections: AdminLibsecretCollectionView[];
  existingAliases: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof aliasSchema>>({
    resolver: zodResolver(aliasSchema),
    defaultValues: {
      name: existingAliases.includes("default") ? "" : "default",
      collection_id: collections[0]?.id ?? "",
    },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof aliasSchema>) =>
      putLibsecretAlias(storeName, resourceId, v.name, {
        collection_id: v.collection_id,
      }),
  });
  async function onSubmit(v: z.infer<typeof aliasSchema>) {
    setError(null);
    try {
      await mut.mutateAsync(v);
      await qc.invalidateQueries({
        queryKey: libsecretTreeKey(storeName, resourceId),
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
          <DialogTitle>Set libsecret alias</DialogTitle>
          <DialogDescription>
            Aliases map a well-known name (commonly <code>default</code>)
            to a collection. libsecret clients call{" "}
            <code>ReadAlias</code> to resolve.
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
            {error ? <Alert>{error}</Alert> : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
