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
import { type AdminUserListItem, postUserResetPassword } from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

const resetSchema = z
  .object({
    password: z.string().min(1, "Password is required."),
    confirm: z.string(),
  })
  .refine((v) => v.password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type ResetValues = z.infer<typeof resetSchema>;

export function ResetPasswordDialog({
  user,
  onClose,
}: {
  user: AdminUserListItem;
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<ResetValues>({
    schema: resetSchema,
    defaultValues: { password: "", confirm: "" },
    submit: (v) =>
      postUserResetPassword(user.id, { password: (v as ResetValues).password }),
    invalidate: [["admin", "users"]],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title={`Reset password: ${user.username}`}
      description="Sets a new password for this user. All of the user's current sessions are revoked server-side as soon as the change lands."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="password"
            render={({ field }) => (
              <FormItem>
                <FormLabel>New password</FormLabel>
                <FormControl>
                  <Input type="password" autoComplete="new-password" {...field} />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="confirm"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Confirm new password</FormLabel>
                <FormControl>
                  <Input type="password" autoComplete="new-password" {...field} />
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
              {pending ? "Resetting…" : "Reset password"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
