import { z } from "zod";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
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
import { Textarea } from "@/components/ui/textarea";
import {
  type AdminClientInstanceListItem,
  type AdminStoresListResponse,
  putClientInstanceUpdate,
} from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

import { AutoCreateField } from "./auto-create-field";

const schema = z.object({
  display_name: z.string().trim().min(1, "Display name is required."),
  current_store_id: z.string().min(1, "Pick a target store."),
  cert_pem: z.string(),
  auto_create_wrapper_assets: z.boolean(),
});
type Values = z.infer<typeof schema>;

type UpdateBody = {
  display_name?: string;
  current_store_id?: string;
  cert_pem?: string;
  auto_create_wrapper_assets?: boolean;
};

export function EditClientInstanceDialog({
  instance,
  stores,
  onClose,
}: {
  instance: AdminClientInstanceListItem;
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
}) {
  function changes(values: Values): UpdateBody {
    const body: UpdateBody = {};
    const name = values.display_name.trim();
    if (name !== instance.display_name) body.display_name = name;
    if (values.current_store_id !== instance.current_store_id)
      body.current_store_id = values.current_store_id;
    if (values.cert_pem.trim() !== "") body.cert_pem = values.cert_pem.trim();
    if (
      values.auto_create_wrapper_assets !==
      (instance.auto_create_wrapper_assets ?? false)
    ) {
      body.auto_create_wrapper_assets = values.auto_create_wrapper_assets;
    }
    return body;
  }
  const { form, onSubmit, pending, submitError } = useFormDialog<
    Values,
    UpdateBody
  >({
    schema,
    defaultValues: {
      display_name: instance.display_name,
      current_store_id: instance.current_store_id,
      cert_pem: "",
      auto_create_wrapper_assets: instance.auto_create_wrapper_assets ?? false,
    },
    toRequest: changes,
    // A programmatic pristine submission is also a local no-op, not a failed request.
    submit: (body) =>
      Object.keys(body).length === 0
        ? Promise.resolve(instance)
        : putClientInstanceUpdate(instance.id, body),
    invalidate: [
      ["admin", "client-instances"],
      ["admin", "stores"],
      ["admin", "wrapper-permissions-view"],
    ],
    onSuccess: onClose,
  });

  const values = form.watch();
  const hasChanges = Object.keys(changes(values)).length > 0;
  const remapping = values.current_store_id !== instance.current_store_id;
  const targetStore = stores.find(
    (store) => store.id === values.current_store_id,
  );

  return (
    <AdminDialog
      open
      nonDismissible={pending}
      onClose={onClose}
      title={`Edit: ${instance.display_name}`}
      description="Rename, remap the target store, configure wrapper-asset creation, or replace the registered certificate. If you replace the certificate, complete the restart operation described beside that field."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} noValidate>
          <fieldset disabled={pending} className="min-w-0 space-y-3">
            <legend className="sr-only">Client instance metadata</legend>
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
            {remapping ? (
              <Alert variant="warning" data-testid="client-remap-impact">
                <strong>
                  {instance.current_store_name} →{" "}
                  {targetStore?.name ?? values.current_store_id}
                </strong>
                <p>
                  This moves runtime traffic from {instance.current_store_id} to{" "}
                  {values.current_store_id}. The client-wide override follows
                  the instance. Check the destination's registered pairs and
                  permissions before Save.
                </p>
                <p>
                  Removal of this client's old-store permission rows is best
                  effort. Inspect the old store afterward; success does not
                  certify cleanup.
                </p>
              </Alert>
            ) : null}
            <FormField
              control={form.control}
              name="cert_pem"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>New certificate PEM (optional)</FormLabel>
                  <FormControl>
                    <Textarea
                      rows={8}
                      spellCheck={false}
                      placeholder={
                        "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"
                      }
                      className="font-mono text-xs"
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>
                    Leave blank to keep the existing certificate. To replace it,
                    complete this operation in order:
                    <span className="mt-1 block pl-2">
                      1. On the frontend, successfully run{" "}
                      <code>
                        {
                          "underwrap-keygen --instance-config /etc/underwrap/daemon/<instance>.yaml"
                        }
                      </code>
                      .
                    </span>
                    <span className="block pl-2">
                      2. Paste the public certificate here and select{" "}
                      <b>Save</b> on this same client instance.
                    </span>
                    <span className="block pl-2">
                      3. Run{" "}
                      <code>
                        {"systemctl restart underwrapd-<instance>.service"}
                      </code>
                      .
                    </span>
                    <span className="block pl-2">
                      4. Verify a representative wrapper.
                    </span>
                    <span className="mt-1 block">
                      The running daemon continues using its cached old identity
                      until restart. After Save and before restart, expect an
                      interruption: wrapper requests fail with enrollment
                      guidance.
                    </span>
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="auto_create_wrapper_assets"
              render={({ field }) => (
                <AutoCreateField
                  checked={field.value}
                  onChange={field.onChange}
                  testId="edit-auto-create"
                />
              )}
            />
            {submitError ? <Alert>{submitError}</Alert> : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={pending || !hasChanges}>
                {pending ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </fieldset>
        </form>
      </Form>
    </AdminDialog>
  );
}
