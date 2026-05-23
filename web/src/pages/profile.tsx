import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
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
  postPasswordChange,
  type WhoamiResponse,
} from "@/api/client";
import { useDocumentTitle } from "@/hooks/use-document-title";
import { describeApiError } from "@/lib/error-message";

interface ProfilePageProps {
  /** Whoami payload from the router context. */
  principal: WhoamiResponse | null;
}

const schema = z
  .object({
    current_password: z.string().min(1, "Current password is required."),
    new_password: z.string().min(8, "Use at least 8 characters."),
    confirm: z.string(),
  })
  .refine((v) => v.new_password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type Values = z.infer<typeof schema>;

export function ProfilePage({ principal }: ProfilePageProps) {
  useDocumentTitle("underwrap — profile");

  const [submitError, setSubmitError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { current_password: "", new_password: "", confirm: "" },
  });
  const mut = useMutation({
    mutationFn: (v: Values) =>
      postPasswordChange({
        current_password: v.current_password,
        new_password: v.new_password,
      }),
    onSuccess: () => {
      setSuccess(true);
      setSubmitError(null);
      form.reset();
    },
    onError: (err) => {
      setSuccess(false);
      setSubmitError(describeApiError(err));
    },
  });

  return (
    <div className="space-y-6">
      <header>
        <h2 className="text-xl font-semibold text-(--color-fg)">Profile</h2>
        <p className="mt-1 text-sm text-(--color-muted)">
          Signed in as{" "}
          <strong className="text-(--color-fg)">
            {principal?.username ?? "(unknown)"}
          </strong>
          .
        </p>
      </header>

      <section className="space-y-4 rounded-xl border border-(--color-line) p-6">
        <div>
          <h3 className="text-base font-semibold text-(--color-fg)">
            Change password
          </h3>
          <p className="mt-1 text-xs text-(--color-muted)">
            You will stay signed in. Other sessions you own are not
            revoked by this self-service flow — use sign-out
            elsewhere if you need that.
          </p>
        </div>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSuccess(false);
              mut.mutate(v);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="current_password"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Current password</FormLabel>
                  <FormControl>
                    <Input type="password" autoComplete="current-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="new_password"
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
            {success ? (
              <Alert variant="success">Password updated.</Alert>
            ) : null}
            <Button type="submit" disabled={mut.isPending}>
              {mut.isPending ? "Saving…" : "Save"}
            </Button>
          </form>
        </Form>
      </section>
    </div>
  );
}
