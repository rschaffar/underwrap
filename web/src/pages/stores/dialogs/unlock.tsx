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
import {
  type AdminStoreListItem,
  postStoreUnlock,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

const unlockSchema = z.object({
  key_hex: z
    .string()
    .trim()
    .regex(/^[0-9a-fA-F]{64}$/, "Master key must be exactly 64 hex characters."),
});
type UnlockValues = z.infer<typeof unlockSchema>;

/**
 * Dialog for unlocking an encrypted store. The operator pastes the
 * 64-character hex master key that was issued at create / rotate
 * time. The hex value lives only in the form state for the duration
 * of the dialog and is wiped via `form.reset()` on success or
 * dismissal — never logged, persisted, or echoed back to the server
 * outside the unlock request itself.
 */
export function UnlockStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<UnlockValues>({
    resolver: zodResolver(unlockSchema),
    defaultValues: { key_hex: "" },
  });
  const unlock = useMutation({
    mutationFn: (key_hex: string) => postStoreUnlock(store.id, { key_hex }),
    onSuccess: async () => {
      form.reset({ key_hex: "" });
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) {
          form.reset({ key_hex: "" });
          onClose();
        }
      }}
    >
      <DialogContent data-testid="store-unlock-dialog">
        <DialogHeader>
          <DialogTitle>Unlock store: {store.name}</DialogTitle>
          <DialogDescription>
            Paste the 64-character hex master key issued when this
            store was created (or last rotated). Until the store is
            unlocked, wrapper requests against it fail with
            <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">store_locked</code>.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              unlock.mutate(v.key_hex.trim());
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="key_hex"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Master key (hex)</FormLabel>
                  <FormControl>
                    <Input
                      autoFocus
                      autoComplete="off"
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      data-testid="store-unlock-key"
                      className="font-mono text-xs"
                      placeholder={"0".repeat(64)}
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {submitError ? (
              <Alert data-testid="store-unlock-error">{submitError}</Alert>
            ) : null}
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => {
                  form.reset({ key_hex: "" });
                  onClose();
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                data-testid="store-unlock-submit"
                disabled={unlock.isPending}
              >
                {unlock.isPending ? "Unlocking…" : "Unlock"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
