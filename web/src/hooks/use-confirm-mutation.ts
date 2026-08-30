import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { describeApiError } from "@/lib/error-message";
import { invalidateAll } from "@/lib/query";

export interface UseConfirmMutationOptions<TArgs = void, TResult = unknown> {
  /** The actual network call. Throws on failure. */
  mutationFn: (args: TArgs) => Promise<TResult>;
  /** Query keys to invalidate after a successful mutation. */
  invalidate?: ReadonlyArray<readonly unknown[]>;
  /** Called on success after invalidation completes. */
  onSuccess?: () => void | Promise<void>;
}

export type ConfirmMutationOutcome<TResult> =
  { status: "success"; result: TResult } | { status: "error" };

interface RequestErrorOutcome {
  status: "error";
  error: unknown;
}

type InternalMutationOutcome<TResult> =
  { status: "success"; result: TResult } | RequestErrorOutcome;

export interface UseConfirmMutationResult<TArgs = void, TResult = unknown> {
  /** Resolve expected request failures as an error outcome; reject callback failures. */
  confirm: (args: TArgs) => Promise<ConfirmMutationOutcome<TResult>>;
  pending: boolean;
  submitError: string | null;
}

/**
 * Companion to [`useFormDialog`] for confirmation dialogs (delete,
 * disable, lock, …). Bundles the standard mutation + invalidate +
 * error-display pattern so the SPA's confirm dialogs stop maintaining
 * their own per-call state machine.
 */
export function useConfirmMutation<TArgs = void, TResult = unknown>(
  opts: UseConfirmMutationOptions<TArgs, TResult>,
): UseConfirmMutationResult<TArgs, TResult> {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation<InternalMutationOutcome<TResult>, unknown, TArgs>({
    mutationFn: async (args) => {
      try {
        return { status: "success", result: await opts.mutationFn(args) };
      } catch (error) {
        return { status: "error", error };
      }
    },
    onSuccess: async (outcome) => {
      if (outcome.status === "error") {
        setSubmitError(describeApiError(outcome.error));
        return;
      }
      if (opts.invalidate) {
        await invalidateAll(qc, opts.invalidate);
      }
      await opts.onSuccess?.();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return {
    confirm: async (args: TArgs) => {
      setSubmitError(null);
      const outcome = await mut.mutateAsync(args);
      return outcome.status === "error" ? { status: "error" } : outcome;
    },
    pending: mut.isPending,
    submitError,
  };
}
