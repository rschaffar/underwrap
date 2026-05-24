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
import {
  type AdminClientInstanceListItem,
  type AdminStoresListResponse,
  putClientInstanceUpdate,
} from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

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
      description="Rename, remap the target store, or replace the registered certificate. Changes take effect on the next runtime request."
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
                      When on, this instance's wrappers may add new
                      resources and wrapper/resource pairs on startup.
                    </FormDescription>
                  </span>
                </label>
              </FormItem>
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
