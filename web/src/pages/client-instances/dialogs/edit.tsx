import { useMutation } from "@tanstack/react-query";
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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import {
  type AdminClientInstanceListItem,
  type AdminStoresListResponse,
  putClientInstanceUpdate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

const schema = z.object({
  display_name: z.string().trim().min(1, "Display name is required."),
  current_store_id: z.string().min(1, "Pick a target store."),
  cert_pem: z.string(),
  auto_create_wrapper_assets: z.boolean(),
});
type Values = z.infer<typeof schema>;

export function EditClientInstanceDialog({
  instance,
  stores,
  onClose,
  onSuccess,
}: {
  instance: AdminClientInstanceListItem;
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
  onSuccess: () => Promise<void>;
}) {
  const defaults: Values = {
    display_name: instance.display_name,
    current_store_id: instance.current_store_id,
    cert_pem: "",
    auto_create_wrapper_assets:
      instance.auto_create_wrapper_assets ?? false,
  };
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: defaults,
  });

  const mut = useMutation({
    mutationFn: (body: {
      display_name?: string;
      current_store_id?: string;
      cert_pem?: string;
      auto_create_wrapper_assets?: boolean;
    }) => putClientInstanceUpdate(instance.id, body),
    onSuccess: async () => {
      await onSuccess();
    },
  });

  async function onSubmit(values: Values) {
    form.clearErrors("root");
    const trimmedDisplayName = values.display_name.trim();
    const trimmedCert = values.cert_pem.trim();
    const displayNameChanged = trimmedDisplayName !== instance.display_name;
    const storeChanged = values.current_store_id !== instance.current_store_id;
    const certChanged = trimmedCert !== "";
    const autoCreateChanged =
      values.auto_create_wrapper_assets !==
      (instance.auto_create_wrapper_assets ?? false);
    if (
      !displayNameChanged &&
      !storeChanged &&
      !certChanged &&
      !autoCreateChanged
    ) {
      form.setError("root", { message: "Nothing to change." });
      return;
    }
    try {
      await mut.mutateAsync({
        ...(displayNameChanged ? { display_name: trimmedDisplayName } : {}),
        ...(storeChanged ? { current_store_id: values.current_store_id } : {}),
        ...(certChanged ? { cert_pem: trimmedCert } : {}),
        ...(autoCreateChanged
          ? { auto_create_wrapper_assets: values.auto_create_wrapper_assets }
          : {}),
      });
    } catch (e) {
      form.setError("root", { message: describeApiError(e) });
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit: {instance.display_name}</DialogTitle>
          <DialogDescription>
            Rename, remap the target store, or replace the registered
            certificate. Changes take effect on the next runtime request.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-3"
            noValidate
          >
            <div className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-3 py-2 text-xs text-(--color-muted)">
              Current fingerprint:{" "}
              <code>{instance.current_credential_fingerprint}</code>
            </div>
            <FormField
              control={form.control}
              name="display_name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Display name</FormLabel>
                  <FormControl>
                    <Input placeholder="laptop-work" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="current_store_id"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Target store</FormLabel>
                  <FormControl>
                    <Select {...field}>
                      {stores.map((store) => (
                        <option key={store.id} value={store.id}>
                          {store.name}
                          {store.id === instance.current_store_id
                            ? " (current)"
                            : ""}
                        </option>
                      ))}
                    </Select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="cert_pem"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>New certificate PEM (optional)</FormLabel>
                  <FormControl>
                    <textarea
                      rows={8}
                      spellCheck={false}
                      placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
                      className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Generate a new credential on the client host with{" "}
                    <code>underwrap-keygen</code> and paste the cert PEM to
                    rotate. Leave blank to keep the existing cert.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="auto_create_wrapper_assets"
              render={({ field }) => (
                <FormItem>
                  <label className="flex items-start gap-2 text-xs text-(--color-muted)">
                    <FormControl>
                      <input
                        type="checkbox"
                        data-testid="edit-auto-create"
                        checked={field.value}
                        onChange={(e) => field.onChange(e.target.checked)}
                        className="mt-0.5"
                      />
                    </FormControl>
                    <span>
                      <strong className="text-(--color-fg)">
                        Auto-create wrapper assets on register
                      </strong>
                      <br />
                      <FormDescription>
                        When on, this instance's wrappers may add new resources
                        and wrapper/resource pairs on startup.
                      </FormDescription>
                    </span>
                  </label>
                </FormItem>
              )}
            />
            {form.formState.errors.root ? (
              <Alert>{form.formState.errors.root.message}</Alert>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
