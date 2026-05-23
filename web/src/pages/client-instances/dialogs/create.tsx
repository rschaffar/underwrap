import { useMutation, useQueryClient } from "@tanstack/react-query";
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
  type AdminClientInstanceCreateRequest,
  type AdminStoresListResponse,
  postClientInstanceCreate,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

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
  const qc = useQueryClient();
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      display_name: "",
      current_store_id: stores[0]?.id ?? "",
      status: "active",
      cert_pem: "",
      auto_create_wrapper_assets: false,
    },
  });

  const mut = useMutation({
    mutationFn: (body: AdminClientInstanceCreateRequest) =>
      postClientInstanceCreate(body),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "client-instances"] });
      onClose();
    },
  });

  async function onSubmit(values: Values) {
    form.clearErrors("root");
    try {
      await mut.mutateAsync({
        display_name: values.display_name.trim(),
        current_store_id: values.current_store_id,
        status: values.status,
        cert_pem: values.cert_pem.trim(),
        auto_create_wrapper_assets: values.auto_create_wrapper_assets
          ? true
          : undefined,
      });
    } catch (e) {
      form.setError("root", { message: describeApiError(e) });
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create client instance</DialogTitle>
          <DialogDescription>
            Generate a credential on the client host with
            <code className="mx-1">underwrap-keygen</code>
            and paste the certificate PEM below. The private key never leaves the
            client host; only the certificate is registered here.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-3"
            noValidate
          >
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
                    <textarea
                      rows={8}
                      spellCheck={false}
                      placeholder={"-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----"}
                      className="flex w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 font-mono text-xs text-(--color-fg)"
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
                <FormItem>
                  <label className="flex items-start gap-2 text-xs text-(--color-muted)">
                    <FormControl>
                      <input
                        type="checkbox"
                        data-testid="create-auto-create"
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
                        and wrapper/resource pairs on startup. When off, the
                        backend refuses to create them and logs which entries it
                        expected.
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
                {mut.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
