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
import { Textarea } from "@/components/ui/textarea";
import {
  type AdminClientInstanceCreateRequest,
  type AdminStoresListResponse,
  postClientInstanceCreate,
} from "@/api/client";
import { useFormDialog } from "@/hooks/use-form-dialog";

import { AutoCreateField } from "./auto-create-field";

const schema = z.object({
  display_name: z.string().trim().min(1, "Display name is required."),
  current_store_id: z.string().min(1, "Pick a target store."),
  status: z.enum(["active", "disabled"]),
  cert_pem: z.string().trim().min(1, "Certificate PEM is required."),
  auto_create_wrapper_assets: z.boolean(),
});
type Values = z.infer<typeof schema>;

export function CreateClientInstanceDialog({
  stores,
  onClose,
}: {
  stores: AdminStoresListResponse["items"];
  onClose: () => void;
}) {
  const { form, onSubmit, pending, submitError } = useFormDialog<
    Values,
    AdminClientInstanceCreateRequest
  >({
    schema,
    defaultValues: {
      display_name: "",
      current_store_id: stores[0]?.id ?? "",
      status: "active",
      cert_pem: "",
      auto_create_wrapper_assets: false,
    },
    toRequest: (v) => ({
      display_name: v.display_name.trim(),
      current_store_id: v.current_store_id,
      status: v.status,
      cert_pem: v.cert_pem.trim(),
      auto_create_wrapper_assets: v.auto_create_wrapper_assets,
    }),
    submit: (req) => postClientInstanceCreate(req),
    invalidate: [["admin", "client-instances"]],
    onSuccess: onClose,
  });
  return (
    <AdminDialog
      open
      onClose={onClose}
      title="Create client instance"
      description={
        <>
          Generate a credential on the client host with
          <code className="mx-1">underwrap-keygen</code>
          and paste the certificate PEM below. The private key never
          leaves the client host; only the certificate is registered
          here.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} className="space-y-3" noValidate>
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
            name="status"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Status</FormLabel>
                <FormControl>
                  <Select {...field}>
                    <option value="active">Active</option>
                    <option value="disabled">Disabled</option>
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
                <FormLabel>Certificate PEM</FormLabel>
                <FormControl>
                  <Textarea
                    rows={8}
                    spellCheck={false}
                    placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
                    className="font-mono text-xs"
                    {...field}
                  />
                </FormControl>
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
                testId="create-auto-create"
                extended
              />
            )}
          />
          {submitError ? <Alert>{submitError}</Alert> : null}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending}>
              {pending ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </Form>
    </AdminDialog>
  );
}
