import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
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
import { Select } from "@/components/ui/select";
import {
  type AdminStoreCreateRequest,
  postStoreCreate,
  postStoreKeyGenerate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";
import { invalidateAllInBackground } from "@/lib/query";

import { KeyConfirmationDialog } from "./key-confirmation";

const createSchema = z.object({
  name: z.string().trim().min(1, "Store name is required."),
  backend: z.enum(["plain", "encrypted"]),
  description: z.string().optional(),
});
type CreateValues = z.infer<typeof createSchema>;

export function CreateStoreDialog({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: { name: "", backend: "plain", description: "" },
  });
  const [draft, setDraft] = useState<CreateValues | null>(null);
  const [generatedKey, setGeneratedKey] = useState<string | null>(null);
  const [generationPending, setGenerationPending] = useState(false);
  const [mutationPending, setMutationPending] = useState(false);
  const [mutationAttempted, setMutationAttempted] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [plainAmbiguous, setPlainAmbiguous] = useState(false);

  function refreshStoresInBackground() {
    invalidateAllInBackground(queryClient, [["admin", "stores"]]);
  }

  function requestFor(values: CreateValues): AdminStoreCreateRequest {
    const body: AdminStoreCreateRequest = {
      name: values.name,
      backend: values.backend,
    };
    if (values.description) body.description = values.description;
    return body;
  }

  const onSubmit = form.handleSubmit(async (values) => {
    form.clearErrors("root");
    setSubmitError(null);
    setPlainAmbiguous(false);

    if (values.backend === "encrypted") {
      setGenerationPending(true);
      try {
        const result = await postStoreKeyGenerate();
        setDraft(values);
        setGeneratedKey(result.key_hex);
      } catch (error) {
        setSubmitError(describeApiError(error));
      } finally {
        setGenerationPending(false);
      }
      return;
    }

    setMutationPending(true);
    try {
      await postStoreCreate(requestFor(values));
      refreshStoresInBackground();
      onClose();
    } catch (error) {
      setPlainAmbiguous(true);
      setSubmitError(describeApiError(error));
      refreshStoresInBackground();
    } finally {
      setMutationPending(false);
    }
  });

  async function createEncrypted() {
    if (!draft || !generatedKey) return;
    setMutationAttempted(true);
    setMutationPending(true);
    setSubmitError(null);
    const request = requestFor(draft);
    request.key_hex = generatedKey;
    try {
      await postStoreCreate(request);
      delete request.key_hex;
      setGeneratedKey(null);
      setDraft(null);
      setMutationAttempted(false);
      refreshStoresInBackground();
      onClose();
    } catch (error) {
      delete request.key_hex;
      setSubmitError(describeApiError(error));
      refreshStoresInBackground();
    } finally {
      setMutationPending(false);
    }
  }

  function clearGeneratedAndClose() {
    setGeneratedKey(null);
    setDraft(null);
    setSubmitError(null);
    setMutationAttempted(false);
    onClose();
  }

  if (generatedKey && draft) {
    return (
      <KeyConfirmationDialog
        title={`Confirm encryption key for ${draft.name}`}
        description="The store does not exist yet. Save and exactly re-enter this fresh backend-generated key before the browser sends the create request."
        keyHex={generatedKey}
        actionLabel="Create encrypted store"
        pendingLabel="Creating…"
        pending={mutationPending}
        mutationAttempted={mutationAttempted}
        submitError={submitError}
        onConfirm={() => void createEncrypted()}
        onCancel={clearGeneratedAndClose}
        onDiscard={clearGeneratedAndClose}
      />
    );
  }

  const backend = form.watch("backend");
  const pending = generationPending || mutationPending;

  return (
    <AdminDialog
      open
      nonDismissible={pending}
      onClose={onClose}
      title="Create store"
      description={
        <>
          The backend allocates the on-disk location under its configured
          <code className="mx-1 rounded bg-(--color-surface-1) px-1 py-0.5 text-xs">
            stores_data_root
          </code>
          .
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-4" noValidate>
          <FormField
            control={form.control}
            name="name"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Name</FormLabel>
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
          <FormField
            control={form.control}
            name="backend"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Backend</FormLabel>
                <FormControl>
                  <Select {...field}>
                    <option value="plain">plain</option>
                    <option value="encrypted">encrypted</option>
                  </Select>
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          {backend === "encrypted" ? (
            <Alert variant="warning">
              The browser first requests a fresh key without creating the
              store. Creation is sent only after you save and exactly re-enter
              that untrimmed key. The backend never writes the key to disk.
            </Alert>
          ) : null}
          <FormField
            control={form.control}
            name="description"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Description (optional)</FormLabel>
                <FormControl>
                  <Input {...field} />
                </FormControl>
                <FormMessage />
              </FormItem>
            )}
          />
          {submitError ? <Alert>{submitError}</Alert> : null}
          {plainAmbiguous ? (
            <Alert variant="warning">
              The browser attempted to submit the create request, and it may
              have reached the backend. An error or lost response cannot prove
              that no store was created. Reconcile the Stores list before
              retrying.
            </Alert>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending}>
              {pending
                ? backend === "encrypted"
                  ? "Generating…"
                  : "Creating…"
                : backend === "encrypted"
                  ? "Generate and review key"
                  : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
