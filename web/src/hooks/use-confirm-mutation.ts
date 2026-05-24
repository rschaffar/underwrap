import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { describeApiError } from "@/lib/error-message";

export interface UseConfirmMutationOptions<TArgs = void> {
  /** The actual network call. Throws on failure. */
  mutationFn: (args: TArgs) => Promise<unknown>;
  /** Query keys to invalidate after a successful mutation. */
  invalidate?: ReadonlyArray<readonly unknown[]>;
  /** Called on success after invalidation completes. */
  onSuccess?: () => void;
}

export interface UseConfirmMutationResult<TArgs = void> {
  /** Invoke the underlying mutation; resolves/rejects with the result. */
  confirm: (args: TArgs) => Promise<unknown>;
  pending: boolean;
  submitError: string | null;
  /** Reset the displayed error (e.g. when re-opening the dialog). */
  clearError: () => void;
}

/**
 * Companion to [`useFormDialog`] for confirmation dialogs (delete,
 * disable, lock, …). Bundles the standard mutation + invalidate +
 * error-display pattern so the SPA's confirm dialogs stop maintaining
 * their own per-call state machine.
 */
export function useConfirmMutation<TArgs = void>(
  opts: UseConfirmMutationOptions<TArgs>,
): UseConfirmMutationResult<TArgs> {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: opts.mutationFn,
    onSuccess: async () => {
      if (opts.invalidate) {
        await Promise.all(
          opts.invalidate.map((key) => qc.invalidateQueries({ queryKey: key })),
        );
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
    clearError: () => setSubmitError(null),
  };
}
