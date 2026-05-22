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
  type AdminStoreCreateRequest,
  postStoreCreate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

import { KeyRevealDialog } from "./key-reveal";

const createSchema = z.object({
  name: z.string().trim().min(1, "Store name is required."),
  backend: z.enum(["plain", "encrypted"]),
  description: z.string().optional(),
});
type CreateValues = z.infer<typeof createSchema>;

export function CreateStoreDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  // Held only while the reveal modal is up; cleared when the operator
  // confirms acknowledgement and we close the modal. Never persisted
  // beyond React state — no localStorage / sessionStorage / URL.
  const [revealKey, setRevealKey] = useState<{
    storeName: string;
    keyHex: string;
  } | null>(null);
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      name: "",
      backend: "plain",
      description: "",
    },
  });

  const create = useMutation({
    mutationFn: (body: AdminStoreCreateRequest) => postStoreCreate(body),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      if (res.key_hex) {
        // Encrypted store: surface the master key once. We keep the
        // create dialog mounted underneath so onClose isn't called
        // until the operator finishes the reveal flow.
        setRevealKey({ storeName: res.store.name, keyHex: res.key_hex });
      } else {
        onClose();
      }
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onSubmit(values: CreateValues) {
    setSubmitError(null);
    const body: AdminStoreCreateRequest = {
      name: values.name,
      backend: values.backend,
    };
    if (values.description) body.description = values.description;
    create.mutate(body);
  }

  if (revealKey) {
    return (
      <KeyRevealDialog
        title={`Encryption key for ${revealKey.storeName}`}
        description={
          <>
            This is the only time the master key will be shown. The
            backend keeps it only in process memory — copy it into a
            password manager or other offline vault now, because you
            will need to paste it back to unlock this store after every
            backend restart.
          </>
        }
        keyHex={revealKey.keyHex}
        onClose={() => {
          setRevealKey(null);
          onClose();
        }}
      />
    );
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create store</DialogTitle>
          <DialogDescription>
            The backend allocates the on-disk location under its configured
            <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">stores_data_root</code>.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4" noValidate>
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Name</FormLabel>
                  <FormControl>
                    <Input autoFocus autoCapitalize="off" autoCorrect="off" spellCheck={false} {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="backend"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Backend</FormLabel>
                  <FormControl>
                    <Select {...field}>
                      <option value="plain">plain</option>
                      <option value="encrypted">encrypted</option>
                    </Select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {form.watch("backend") === "encrypted" ? (
              <Alert variant="warning">
                The backend will generate a fresh 32-byte master key
                in memory and show it to you once after creation. The
                key is <strong>never written to disk</strong>; you must
                save it somewhere safe yourself. After any backend
                restart the store is locked until an admin re-enters
                this key.
              </Alert>
            ) : null}
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Description (optional)</FormLabel>
                  <FormControl>
                    <Input {...field} />
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
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
