import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

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
import { Button } from "@/components/ui/button";
import { postBootstrap } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

const MIN_PASSWORD_LEN = 12;

const schema = z
  .object({
    username: z.string().min(1, "Username is required."),
    password: z
      .string()
      .min(MIN_PASSWORD_LEN, `Password must be at least ${MIN_PASSWORD_LEN} characters.`),
    confirm: z.string().min(1, "Please re-enter the password."),
  })
  .refine((v) => v.password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type BootstrapValues = z.infer<typeof schema>;

interface BootstrapPageProps {
  onSuccess: () => void | Promise<void>;
}

export function BootstrapPage({ onSuccess }: BootstrapPageProps) {
  const [submitError, setSubmitError] = useState<string | null>(null);

  useEffect(() => {
    document.title = "underwrap — first-time setup";
  }, []);

  const form = useForm<BootstrapValues>({
    resolver: zodResolver(schema),
    defaultValues: { username: "", password: "", confirm: "" },
    mode: "onSubmit",
  });

  async function onSubmit(values: BootstrapValues) {
    setSubmitError(null);
    try {
      await postBootstrap({
        username: values.username,
        password: values.password,
      });
      await onSuccess();
    } catch (err) {
      setSubmitError(describeApiError(err));
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6 shadow-sm">
        <header className="mb-5">
          <h1 className="text-lg font-semibold text-(--color-fg)">
            First-time setup
          </h1>
          <p className="mt-1 text-sm text-(--color-muted)">
            Create the first admin operator for this backend. After this
            you'll be signed in immediately.
          </p>
        </header>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="username"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Username</FormLabel>
                  <FormControl>
                    <Input
                      autoComplete="username"
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                      autoFocus
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Stored lower-cased. Choose something you'll remember.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="password"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Password</FormLabel>
                  <FormControl>
                    <Input
                      type="password"
                      autoComplete="new-password"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    At least {MIN_PASSWORD_LEN} characters.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirm"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Confirm password</FormLabel>
                  <FormControl>
                    <Input
                      type="password"
                      autoComplete="new-password"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {submitError ? (
              <p
                role="alert"
                className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
              >
                {submitError}
              </p>
            ) : null}
            <Button
              type="submit"
              className="w-full"
              disabled={form.formState.isSubmitting}
            >
              {form.formState.isSubmitting
                ? "Creating admin…"
                : "Create first admin"}
            </Button>
          </form>
        </Form>
      </div>
    </div>
  );
}
