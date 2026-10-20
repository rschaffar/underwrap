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
import { type AdminStoreListItem, postStoreUnlock } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const unlockSchema = z.object({
  key_hex: z
    .string()
    .trim()
    .regex(/^[0-9a-fA-F]{64}$/, "Master key must be exactly 64 hex characters."),
});
type UnlockValues = z.infer<typeof unlockSchema>;

/**
 * Dialog for unlocking an encrypted store. The hex value lives only in
 * the form state for the duration of the dialog — never logged,
 * persisted, or echoed back to the server outside the unlock request
 * itself.
 */
export function UnlockStoreDialog({
  store,
  onClose,
}: {
  store: AdminStoreListItem;
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<UnlockValues>({
    schema: unlockSchema,
    defaultValues: { key_hex: "" },
    submit: (v) => postStoreUnlock(store.id, { key_hex: v.key_hex.trim() }),
    invalidate: [["admin", "stores"]],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      nonDismissible={pending}
      title={`Unlock store: ${store.name}`}
      description={
        <>
          Paste the 64-character hex master key issued when this store
          was created (or last rotated). Until the store is unlocked,
          wrapper requests against it fail with
          <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">store_locked</code>.
        </>
      }
    >
      <Form {...form}>
        <form
          onSubmit={onSubmit}
          className="space-y-4"
          noValidate
          data-testid="store-unlock-dialog"
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
            <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="store-unlock-submit"
              disabled={pending}
            >
              {pending ? "Unlocking…" : "Unlock"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
