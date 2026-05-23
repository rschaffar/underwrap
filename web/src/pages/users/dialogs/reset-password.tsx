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
  type AdminUserListItem,
  postUserResetPassword,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<ResetValues>({
    resolver: zodResolver(resetSchema),
    defaultValues: { password: "", confirm: "" },
  });
  const mut = useMutation({
    mutationFn: (password: string) =>
      postUserResetPassword(user.id, { password }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reset password: {user.username}</DialogTitle>
          <DialogDescription>
            Sets a new password for this user. All of the user's
            current sessions are revoked server-side as soon as the
            change lands.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v.password);
            })}
            className="space-y-4"
            noValidate
          >
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
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Resetting…" : "Reset password"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
