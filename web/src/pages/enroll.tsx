import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

import { Button } from "@/components/ui/button";
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
import { Label } from "@/components/ui/label";
import {
  type AdminEnrollmentInviteCreateRequest,
  type AdminEnrollmentInviteCreateResponse,
  type AdminStoreCreateRequest,
  type AdminStoresListResponse,
  fetchStores,
  postEnrollmentInviteCreate,
  postStoreCreate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

const inviteSchema = z.object({
  display_name: z.string().min(1, "Display name is required."),
  hostname_hint: z.string().optional(),
  instance_name: z.string().min(1, "Instance name is required."),
  current_store_id: z.string().min(1, "Choose a store or create one inline."),
});
type InviteValues = z.infer<typeof inviteSchema>;

const createStoreSchema = z.object({
  name: z
    .string()
    .min(1, "Store name is required.")
    .regex(
      /^[a-z0-9][a-z0-9_-]*$/,
      "Use lowercase letters, digits, '-' or '_'; must start with a letter or digit.",
    ),
  backend: z.enum(["plain", "encrypted"]),
  dir: z.string().min(1, "Data directory is required."),
  description: z.string().optional(),
});
type CreateStoreValues = z.infer<typeof createStoreSchema>;

export interface EnrollmentHints {
  host: string | null;
  instance: string | null;
}

export function parseEnrollmentHints(search: string): EnrollmentHints {
  const params = new URLSearchParams(search.startsWith("?") ? search : `?${search}`);
  return {
    host: params.get("host"),
    instance: params.get("instance"),
  };
}

export function buildEnrollmentCommand(
  enrollmentUrl: string,
  inviteId: string,
  instanceName: string,
  currentOrigin = typeof window !== "undefined" ? window.location.origin : "https://backend.example",
): string {
  const resolvedUrl = new URL(enrollmentUrl, currentOrigin).toString();
  const suffix = instanceName.trim() ? `-${instanceName.trim()}` : "";
  return [
    "sudo underwrap-enroll",
    `--url ${shellQuote(resolvedUrl)}`,
    `--invite-id ${shellQuote(inviteId)}`,
    "--ca-file /etc/underwrap/ca.pem",
    `--client-cert-out /etc/underwrap/client${suffix}.pem`,
    `--client-key-out /etc/underwrap/client${suffix}.key`,
  ].join(" ");
}

function shellQuote(value: string): string {
  if (/^[A-Za-z0-9_./:-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function canCreateStores(principal: WhoamiResponse | null): boolean {
  return !!(principal && principal.authenticated && principal.role === "admin");
}

function storeNameFromList(
  stores: AdminStoresListResponse["items"],
  storeId: string,
): string {
  return stores.find((s) => s.id === storeId)?.name ?? storeId;
}

export function EnrollmentPage({ principal }: { principal: WhoamiResponse | null }) {
  const qc = useQueryClient();
  const hints = useMemo(() => parseEnrollmentHints(window.location.search), []);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [storeCreateError, setStoreCreateError] = useState<string | null>(null);
  const [showCreateStore, setShowCreateStore] = useState(false);
  const [createdStoreKeyHex, setCreatedStoreKeyHex] = useState<string | null>(null);
  const [inviteResult, setInviteResult] = useState<AdminEnrollmentInviteCreateResponse | null>(null);
  const [copyState, setCopyState] = useState<{ secret: boolean; command: boolean }>({
    secret: false,
    command: false,
  });

  useEffect(() => {
    document.title = "underwrap — enrollment";
  }, []);

  const storesQuery = useQuery<AdminStoresListResponse>({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
  });

  const inviteForm = useForm<InviteValues>({
    resolver: zodResolver(inviteSchema),
    defaultValues: {
      display_name: [hints.host, hints.instance].filter(Boolean).join(" ") || "",
      hostname_hint: hints.host ?? "",
      instance_name: hints.instance ?? "",
      current_store_id: "",
    },
  });

  const createStoreForm = useForm<CreateStoreValues>({
    resolver: zodResolver(createStoreSchema),
    defaultValues: {
      name: hints.instance ? `store_${hints.instance}` : "",
      backend: "plain",
      dir: hints.instance
        ? `/var/lib/underwrap-backend/data/${hints.instance}`
        : "/var/lib/underwrap-backend/data/",
      description: hints.host && hints.instance ? `${hints.host} ${hints.instance}` : "",
    },
  });

  useEffect(() => {
    if (!inviteForm.getValues("current_store_id")) {
      const first = storesQuery.data?.items[0]?.id;
      if (first) {
        inviteForm.setValue("current_store_id", first, { shouldValidate: true });
      }
    }
  }, [inviteForm, storesQuery.data]);

  const createStore = useMutation({
    mutationFn: (body: AdminStoreCreateRequest) => postStoreCreate(body),
    onSuccess: async (store) => {
      setStoreCreateError(null);
      setShowCreateStore(false);
      setCreatedStoreKeyHex(store.key_hex ?? null);
      inviteForm.setValue("current_store_id", store.store.id, { shouldValidate: true });
      await qc.invalidateQueries({ queryKey: ["admin", "stores"] });
      createStoreForm.reset({
        name: "",
        backend: "plain",
        dir: "",
        description: "",
      });
    },
    onError: (err) => setStoreCreateError(describeApiError(err)),
  });

  const createInvite = useMutation({
    mutationFn: (body: AdminEnrollmentInviteCreateRequest) => postEnrollmentInviteCreate(body),
    onSuccess: (response) => {
      setSubmitError(null);
      setInviteResult(response);
      setCopyState({ secret: false, command: false });
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });

  function onCreateStore(values: CreateStoreValues) {
    setStoreCreateError(null);
    const body: AdminStoreCreateRequest = {
      name: values.name,
      backend: values.backend,
      dir: values.dir,
    };
    if (values.description) body.description = values.description;
    createStore.mutate(body);
  }

  function onCreateInvite(values: InviteValues) {
    setSubmitError(null);
    createInvite.mutate({
      display_name: values.display_name,
      hostname_hint: values.hostname_hint?.trim() || undefined,
      instance_name: values.instance_name,
      current_store_id: values.current_store_id,
    });
  }

  const selectedStoreId = inviteForm.watch("current_store_id");
  const canCreateInline = canCreateStores(principal);
  const stores = storesQuery.data?.items ?? [];
  const command = inviteResult
    ? buildEnrollmentCommand(
        inviteResult.enrollment_url,
        inviteResult.invite.id,
        inviteResult.invite.instance_name,
      )
    : null;

  async function copyValue(kind: "secret" | "command", value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopyState((current) => ({ ...current, [kind]: true }));
    } catch {
      setCopyState((current) => ({ ...current, [kind]: false }));
    }
  }

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <header className="space-y-2">
        <h2 className="text-xl font-semibold text-(--color-fg)">Enroll instance</h2>
        <p className="text-sm text-(--color-muted)">
          Create an operator-approved one-time invite for this instance, then run the
          local enrollment helper on the client host.
        </p>
      </header>

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <section className="rounded-xl border border-(--color-line) bg-(--color-surface-0)/50 p-5">
          <Form {...inviteForm}>
            <form
              onSubmit={inviteForm.handleSubmit(onCreateInvite)}
              className="space-y-4"
              noValidate
            >
              <FormField
                control={inviteForm.control}
                name="hostname_hint"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Host hint</FormLabel>
                    <FormControl>
                      <Input
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        {...field}
                        value={field.value ?? ""}
                      />
                    </FormControl>
                    <FormDescription>
                      Non-sensitive hint from the wrapper deep link. Used to match re-enrollment for
                      the same logical instance.
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <FormField
                control={inviteForm.control}
                name="instance_name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Instance name</FormLabel>
                    <FormControl>
                      <Input
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        {...field}
                      />
                    </FormControl>
                    <FormDescription>
                      Usually the <code>services.underwrap.instances.&lt;name&gt;</code> value from the
                      wrapper-generated link.
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <FormField
                control={inviteForm.control}
                name="display_name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Display name</FormLabel>
                    <FormControl>
                      <Input {...field} />
                    </FormControl>
                    <FormDescription>
                      Operator-facing label for the enrolled runtime client instance.
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <FormField
                control={inviteForm.control}
                name="current_store_id"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Store</FormLabel>
                    <FormControl>
                      <select
                        aria-label="Store"
                        className="flex h-10 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
                        {...field}
                      >
                        <option value="">Select a store…</option>
                        {stores.map((store) => (
                          <option key={store.id} value={store.id}>
                            {store.name}
                          </option>
                        ))}
                      </select>
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />

              {storesQuery.isLoading ? (
                <p className="text-sm text-(--color-muted)">Loading stores…</p>
              ) : storesQuery.error ? (
                <p role="alert" className="text-sm text-rose-300">
                  {describeApiError(storesQuery.error)}
                </p>
              ) : stores.length === 0 && !canCreateInline ? (
                <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
                  No stores are visible to you. Ask an admin for a store grant or have them create a
                  store inline here.
                </p>
              ) : null}

              {submitError ? (
                <p role="alert" className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
                  {submitError}
                </p>
              ) : null}

              <div className="flex flex-wrap gap-3">
                <Button type="submit" disabled={createInvite.isPending || stores.length === 0 && !canCreateInline}>
                  {createInvite.isPending ? "Creating invite…" : "Create enrollment invite"}
                </Button>
                {canCreateInline ? (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => setShowCreateStore((current) => !current)}
                  >
                    {showCreateStore ? "Hide inline store creation" : "Create new store inline"}
                  </Button>
                ) : null}
              </div>
            </form>
          </Form>
        </section>

        <section className="rounded-xl border border-(--color-line) bg-(--color-surface-0)/50 p-5">
          <h3 className="text-base font-semibold text-(--color-fg)">Run on the client host</h3>
          <p className="mt-1 text-sm text-(--color-muted)">
            Create the invite first. Then run the command below on the machine that owns this
            underwrap instance. The helper prompts interactively for the enrollment secret.
          </p>
          {inviteResult ? (
            <div className="mt-4 space-y-4">
              <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="text-xs font-semibold uppercase tracking-wide text-amber-200">
                      Enrollment secret
                    </p>
                    <p className="mt-1 break-all font-mono text-sm text-(--color-fg)">
                      {inviteResult.enrollment_secret}
                    </p>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => copyValue("secret", inviteResult.enrollment_secret)}
                  >
                    {copyState.secret ? "Copied" : "Copy"}
                  </Button>
                </div>
                <p className="mt-2 text-xs text-amber-200">
                  This value is shown exactly once. Enter it only into the interactive prompt on the
                  client host.
                </p>
              </div>

              <div>
                <div className="flex items-center justify-between gap-3">
                  <Label>Enrollment command</Label>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => command && copyValue("command", command)}
                  >
                    {copyState.command ? "Copied" : "Copy"}
                  </Button>
                </div>
                <pre className="mt-2 overflow-x-auto rounded-md border border-(--color-line) bg-(--color-surface-1) p-3 text-xs text-(--color-fg)">
                  <code>{command}</code>
                </pre>
                <p className="mt-2 text-xs text-(--color-muted)">
                  Target store: {storeNameFromList(stores, inviteResult.invite.current_store_id)} ·
                  instance: {inviteResult.invite.instance_name}
                </p>
              </div>
            </div>
          ) : (
            <p className="mt-4 text-sm text-(--color-muted)">
              No invite created yet.
            </p>
          )}

          {createdStoreKeyHex ? (
            <div className="mt-4 rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3">
              <p className="text-xs font-semibold uppercase tracking-wide text-emerald-200">
                Newly created store key
              </p>
              <p className="mt-1 break-all font-mono text-xs text-(--color-fg)">{createdStoreKeyHex}</p>
              <p className="mt-2 text-xs text-emerald-200">
                The backend returns an encrypted store key exactly once. Save it now if you need a
                disaster-recovery copy.
              </p>
            </div>
          ) : null}
        </section>
      </div>

      {canCreateInline && showCreateStore ? (
        <section className="rounded-xl border border-(--color-line) bg-(--color-surface-0)/50 p-5">
          <h3 className="text-base font-semibold text-(--color-fg)">Create store inline</h3>
          <p className="mt-1 text-sm text-(--color-muted)">
            Admin-only. The newly created store is selected automatically for the enrollment invite.
          </p>
          <Form {...createStoreForm}>
            <form
              onSubmit={createStoreForm.handleSubmit(onCreateStore)}
              className="mt-4 grid gap-4 md:grid-cols-2"
              noValidate
            >
              <FormField
                control={createStoreForm.control}
                name="name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Store name</FormLabel>
                    <FormControl>
                      <Input {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <FormField
                control={createStoreForm.control}
                name="backend"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Backend</FormLabel>
                    <FormControl>
                      <select
                        aria-label="Store backend"
                        className="flex h-10 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
                        {...field}
                      >
                        <option value="plain">plain</option>
                        <option value="encrypted">encrypted</option>
                      </select>
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />

              <div className="md:col-span-2">
                <FormField
                  control={createStoreForm.control}
                  name="dir"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>Data directory</FormLabel>
                      <FormControl>
                        <Input {...field} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>

              <div className="md:col-span-2">
                <FormField
                  control={createStoreForm.control}
                  name="description"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>Description</FormLabel>
                      <FormControl>
                        <Input {...field} value={field.value ?? ""} />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>

              {storeCreateError ? (
                <p role="alert" className="md:col-span-2 rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
                  {storeCreateError}
                </p>
              ) : null}

              <div className="md:col-span-2 flex gap-3">
                <Button type="submit" disabled={createStore.isPending}>
                  {createStore.isPending ? "Creating store…" : "Create store"}
                </Button>
                <Button type="button" variant="ghost" onClick={() => setShowCreateStore(false)}>
                  Cancel
                </Button>
              </div>
            </form>
          </Form>
        </section>
      ) : null}

      {selectedStoreId ? (
        <p className="text-xs text-(--color-muted)">
          Selected store: {storeNameFromList(stores, selectedStoreId)}
        </p>
      ) : null}
    </div>
  );
}
