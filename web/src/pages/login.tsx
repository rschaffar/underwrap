import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Alert } from "@/components/ui/alert";
import { ApiError, postLogin } from "@/api/client";
import { describeApiError } from "@/lib/error-message";

const schema = z.object({
  username: z.string().min(1, "Username is required."),
  password: z.string().min(1, "Password is required."),
});
type LoginValues = z.infer<typeof schema>;

interface LoginPageProps {
  onSuccess: () => void | Promise<void>;
}

export function LoginPage({ onSuccess }: LoginPageProps) {
  const [submitError, setSubmitError] = useState<string | null>(null);

  useEffect(() => {
    document.title = "underwrap — sign in";
  }, []);

  const form = useForm<LoginValues>({
    resolver: zodResolver(schema),
    defaultValues: { username: "", password: "" },
    mode: "onSubmit",
  });

  async function onSubmit(values: LoginValues) {
    setSubmitError(null);
    try {
      await postLogin(values);
      await onSuccess();
    } catch (err) {
      if (err instanceof ApiError && err.code === "invalid_credentials") {
        // Surface this inline on the form rather than via a toast: it's
        // the most likely failure and tied to the fields the user just
        // edited.
        form.setError("password", {
          type: "server",
          message: "Invalid username or password.",
        });
        return;
      }
      setSubmitError(describeApiError(err));
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6 shadow-sm">
        <header className="mb-5">
          <h1 className="text-lg font-semibold text-(--color-fg)">Sign in</h1>
          <p className="mt-1 text-sm text-(--color-muted)">
            Use your underwrap operator credentials.
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
                      autoComplete="current-password"
                      {...field}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {submitError ? <Alert>{submitError}</Alert> : null}
            <Button
              type="submit"
              className="w-full"
              disabled={form.formState.isSubmitting}
            >
              {form.formState.isSubmitting ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </Form>
      </div>
    </div>
  );
}
