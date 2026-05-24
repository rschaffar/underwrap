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
import { postLibsecretItem } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

import { ID_PATTERN, libsecretTreeKey, parseAttributes } from "../internal";

const itemSchema = z.object({
  id: z
    .string()
    .optional()
    .refine(
      (s) => !s || ID_PATTERN.test(s),
      "Id must match [A-Za-z0-9._-]+ and not start with '_'.",
    ),
  label: z.string().min(1, "Label is required."),
  content_type: z.string().min(1, "Content type is required."),
  attributes: z.string(),
  secret: z.string().min(1, "Secret material is required."),
});

export function CreateItemDialog({
  storeName,
  resourceId,
  collectionId,
  existingIds,
  onClose,
}: {
  storeName: string;
  resourceId: string;
  collectionId: string;
  existingIds: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const form = useForm<z.infer<typeof itemSchema>>({
    resolver: zodResolver(itemSchema),
    defaultValues: {
      id: "",
      label: "",
      content_type: "text/plain",
      attributes: "",
      secret: "",
    },
  });
  const mut = useMutation({
    mutationFn: (v: z.infer<typeof itemSchema>) =>
      postLibsecretItem(storeName, resourceId, collectionId, {
        id: v.id || undefined,
        label: v.label,
        content_type: v.content_type,
        attributes: parseAttributes(v.attributes),
        secret_b64: btoa(v.secret),
      }),
  });
  async function onSubmit(v: z.infer<typeof itemSchema>) {
    setError(null);
    if (v.id && existingIds.includes(v.id)) {
      form.setError("id", { message: "Id already exists in this collection." });
      return;
    }
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
          <DialogTitle>Create libsecret item</DialogTitle>
          <DialogDescription>
            In collection <code>{collectionId}</code> of resource{" "}
            <code>{resourceId}</code>. Secret is sent over TLS,
            base64-encoded in transit, and never echoed back.
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
                      placeholder="auto-generated if blank, e.g. item_1"
                      {...field}
                    />
                  </FormControl>
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
                      placeholder="e.g. GitHub Token"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="content_type"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Content type</FormLabel>
                  <FormControl>
                    <Input
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      placeholder="text/plain"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    The MIME-style type libsecret carries in the Secret
                    struct. Most apps use <code>text/plain</code>.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="attributes"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Attributes (optional)</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="libsecret-item-attributes-input"
                      rows={3}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
                      placeholder={
                        "xdg:schema=org.freedesktop.Secret.Generic\nservice=github.com"
                      }
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    One <code>key=value</code> per line. libsecret looks
                    these up case-sensitively.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="secret"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Secret material</FormLabel>
                  <FormControl>
                    <textarea
                      data-testid="libsecret-item-secret-input"
                      rows={4}
                      className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
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
