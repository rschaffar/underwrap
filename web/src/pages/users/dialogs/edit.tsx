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
import { type AdminUserListItem, putUserUpdate } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const editSchema = z.object({
  username: z.string().trim().min(1, "Username is required."),
});
type EditValues = z.infer<typeof editSchema>;

export function EditUserDialog({
  user,
  onClose,
}: {
  user: AdminUserListItem;
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<EditValues>({
    schema: editSchema,
    defaultValues: { username: user.username },
    submit: (v) => putUserUpdate(user.id, { username: (v as EditValues).username }),
    invalidate: [["admin", "users"]],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title={`Edit user: ${user.username}`}
      description="Rename the user. Use the separate “Reset password” action to change a password."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="username"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Username</FormLabel>
                <FormControl>
                  <Input
                    autoFocus
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    {...field}
                  />
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
            <Button type="submit" disabled={pending}>
              {pending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
