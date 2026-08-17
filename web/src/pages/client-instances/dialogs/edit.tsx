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
  const { form, onSubmit, pending, submitError } = useFormDialog<Values, UpdateBody>({
    schema,
    defaultValues: {
      display_name: instance.display_name,
      current_store_id: instance.current_store_id,
      cert_pem: "",
      auto_create_wrapper_assets: instance.auto_create_wrapper_assets ?? false,
    },
    toRequest: (v) => {
      // The hook can't observe `dirtyFields` from inside `toRequest`,
      // so we recompute it here once. The PUT endpoint is idempotent
      // but we still send only the touched keys so audit events stay
      // meaningful.
      const dirty = form.formState.dirtyFields;
      const body: UpdateBody = {};
      if (dirty.display_name) body.display_name = v.display_name.trim();
      if (dirty.current_store_id) body.current_store_id = v.current_store_id;
      if (dirty.cert_pem && v.cert_pem.trim() !== "") {
        body.cert_pem = v.cert_pem.trim();
      }
      if (dirty.auto_create_wrapper_assets) {
        body.auto_create_wrapper_assets = v.auto_create_wrapper_assets;
      }
      return body;
    },
    submit: (body) => {
      if (Object.keys(body).length === 0) {
        throw new Error("Nothing to change.");
      }
      return putClientInstanceUpdate(instance.id, body);
    },
    invalidate: [["admin", "client-instances"]],
    onSuccess: onClose,
  });

  return (
    <AdminDialog
      open
      onClose={onClose}
      title={`Edit: ${instance.display_name}`}
      description="Rename, remap the target store, configure wrapper-asset creation, or replace the registered certificate. If you replace the certificate, complete the restart operation described beside that field."
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-3" noValidate>
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
                  <Textarea
                    rows={8}
                    spellCheck={false}
                    placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
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
                    2. Paste the public certificate here and select <b>Save</b>{" "}
                    on this same client instance.
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
            <Button type="submit" disabled={pending}>
              {pending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
