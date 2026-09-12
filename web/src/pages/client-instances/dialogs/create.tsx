import { z } from "zod";
import { useState } from "react";
import { Link } from "@tanstack/react-router";

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
  type AdminClientInstanceListItem,
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
  initialStoreId,
  onClose,
}: {
  stores: AdminStoresListResponse["items"];
  initialStoreId?: string;
  onClose: () => void;
}) {
  const [created, setCreated] = useState<AdminClientInstanceListItem | null>(
    null,
  );
  const { form, onSubmit, pending, submitError } = useFormDialog<
    Values,
    AdminClientInstanceCreateRequest,
    AdminClientInstanceListItem
  >({
    schema,
    defaultValues: {
      display_name: "",
      current_store_id:
        stores.find((store) => store.id === initialStoreId)?.id ??
        stores[0]?.id ??
        "",
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
    invalidate: [
      ["admin", "client-instances"],
      ["admin", "stores"],
    ],
    onSuccess: setCreated,
  });
  if (created) {
    return (
      <AdminDialog
        open
        onClose={onClose}
        title={`Enrolled: ${created.display_name}`}
        description="The backend has registered this public certificate. Frontend restart and wrapper verification are still required."
      >
        <div className="space-y-3 text-sm">
          {created.status === "disabled" ? (
            <Alert variant="warning">
              This instance is disabled. Enable it before expecting runtime
              requests to succeed.
            </Alert>
          ) : null}
          {created.auto_create_wrapper_assets ? (
            <p>
              Asset auto-creation is enabled. Restart the matching frontend
              daemon so it registers its configured assets, then inspect the
              resources and wrapper/resource pairs.
            </p>
          ) : (
            <p>
              Asset auto-creation is off. Create the resource and
              wrapper/resource pair matching the frontend configuration before
              restarting its daemon.
            </p>
          )}
          <p>
            On the frontend host, run{" "}
            <code>systemctl restart underwrapd-&lt;instance&gt;.service</code>{" "}
            using the actual frontend instance name, not this backend display
            name. Restart interrupts that daemon's traffic.
          </p>
          <p>
            Add the resource values, save an explicit permission, and verify a
            representative real wrapper. Enrollment alone does not prove the
            target works.
          </p>
          <Link
            className="underline"
            to="/setup"
            search={{ store: created.current_store_id, client: created.id }}
          >
            Continue Get a wrapper working
          </Link>
        </div>
        <DialogFooter>
          <Button onClick={onClose}>Done</Button>
        </DialogFooter>
      </AdminDialog>
    );
  }
  return (
    <AdminDialog
      open
      nonDismissible={pending}
      onClose={onClose}
      title="Create client instance"
      description={
        <>
          Generate a credential on the client host with
          <code className="mx-1">underwrap-keygen</code>
          and paste the certificate PEM below. The private key never leaves the
          client host; only the certificate is registered here.
        </>
      }
    >
      <Form {...form}>
        <form onSubmit={onSubmit} noValidate>
          <fieldset disabled={pending} className="min-w-0 space-y-3">
            <legend className="sr-only">Runtime client enrollment</legend>
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
                      placeholder={
                        "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"
                      }
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
          </fieldset>
        </form>
      </Form>
    </AdminDialog>
  );
}
