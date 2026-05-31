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
  onSuccess?: () => void;
}

export interface UseConfirmMutationResult<TArgs = void, TResult = unknown> {
  /** Invoke the underlying mutation; resolves/rejects with the result. */
  confirm: (args: TArgs) => Promise<TResult>;
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
  const mut = useMutation({
    mutationFn: opts.mutationFn,
    onSuccess: async () => {
      if (opts.invalidate) {
        await invalidateAll(qc, opts.invalidate);
      }
      opts.onSuccess?.();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return {
    confirm: (args: TArgs) => {
      setSubmitError(null);
      return mut.mutateAsync(args);
    },
    pending: mut.isPending,
    submitError,
  };
}
