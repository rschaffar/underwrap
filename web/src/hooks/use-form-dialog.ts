import { useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  useForm,
  type DefaultValues,
  type FieldValues,
  type UseFormReturn,
} from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";

import { describeApiError } from "@/lib/error-message";
import { invalidateAll } from "@/lib/query";

export interface UseFormDialogOptions<
  TValues extends FieldValues,
  TRequest,
  TResult,
> {
  // We can't constrain this tightly without bumping into zod-resolver's
  // generic awkwardness; the caller picks the schema and the hook just
  // hands it to `zodResolver` as-is.
  schema: unknown;
  defaultValues: DefaultValues<TValues>;
  /**
   * Maps validated form values to the mutation argument. Optional —
   * defaults to passing the values through untouched.
   */
  toRequest?: (values: TValues) => TRequest;
  /** The actual network call. Throws on failure. */
  submit: (request: TRequest) => Promise<TResult>;
  /** Query keys to invalidate after a successful mutation. */
  invalidate?: ReadonlyArray<readonly unknown[]>;
  /**
   * Called on success after invalidation completes. Receives the
   * mutation's return value so multi-step flows (e.g. create-then-
   * reveal-key) can route off the response without redoing the call.
   */
  onSuccess: (result: TResult) => void | Promise<void>;
}

export interface UseFormDialogResult<TValues extends FieldValues> {
  form: UseFormReturn<TValues>;
  /** Submit handler to pass to `<form onSubmit={...}>`. */
  onSubmit: ReturnType<UseFormReturn<TValues>["handleSubmit"]>;
  /** True through validation, the request, invalidation, and the success callback. */
  pending: boolean;
  /** The "root" submit error message, if any. */
  submitError: string | undefined;
}

/**
 * Serialize the complete submission, including async form validation.
 * Only request failures become form errors; mapping, invalidation, and callback
 * failures remain observable rejections, not misleading network errors.
 */
export function useFormDialog<
  TValues extends FieldValues,
  TRequest = TValues,
  TResult = unknown,
>(
  opts: UseFormDialogOptions<TValues, TRequest, TResult>,
): UseFormDialogResult<TValues> {
  const qc = useQueryClient();
  const inFlight = useRef(false);
  const [pending, setPending] = useState(false);
  const form = useForm<TValues>({
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    resolver: zodResolver(opts.schema as any),
    defaultValues: opts.defaultValues,
  });
  const mut = useMutation({
    mutationFn: (request: TRequest) => opts.submit(request),
  });
  const validatedSubmit = form.handleSubmit(async (values) => {
    form.clearErrors("root");
    const request = opts.toRequest
      ? opts.toRequest(values as TValues)
      : (values as unknown as TRequest);
    let result: TResult;
    try {
      result = await mut.mutateAsync(request);
    } catch (error) {
      form.setError("root", { message: describeApiError(error) });
      return;
    }
    if (opts.invalidate) await invalidateAll(qc, opts.invalidate);
    await opts.onSuccess(result);
  });
  const onSubmit: ReturnType<typeof form.handleSubmit> = async (event) => {
    event?.preventDefault();
    if (inFlight.current) return;
    inFlight.current = true;
    setPending(true);
    try {
      await validatedSubmit(event);
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  };
  return {
    form,
    onSubmit,
    pending,
    submitError: form.formState.errors.root?.message,
  };
}
