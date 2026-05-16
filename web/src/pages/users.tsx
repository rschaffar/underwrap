import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { ShieldCheck, UserCog } from "lucide-react";

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
import { Label } from "@/components/ui/label";
import {
  type AdminUserCreateRequest,
  type AdminUserDetailResponse,
  type AdminUserRole,
  type AdminUsersListResponse,
  deleteUser,
  fetchStores,
  fetchUsers,
  postUserCreate,
  postUserResetPassword,
  putUserUpdate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface UsersPageProps {
  /** Whoami payload from the router context. Used to mark the current
   *  operator with "(you)" and to keep them from removing themselves
   *  as the last admin. */
  principal: WhoamiResponse | null;
}

/** True when the current principal is an admin. Server enforces this
 *  too; this just hides UI controls the user couldn't successfully use. */
export function canManageUsers(p: WhoamiResponse | null): boolean {
  return !!(p && p.authenticated && p.role === "admin");
}

export function UsersPage({ principal }: UsersPageProps) {
  useEffect(() => {
    document.title = "underwrap — users";
  }, []);

  const isAdmin = canManageUsers(principal);
  const meUsername = principal?.username ?? null;

  const { data, error, isLoading } = useQuery<AdminUsersListResponse>({
    queryKey: ["admin", "users"],
    queryFn: fetchUsers,
    enabled: isAdmin,
  });
  // The grant editor needs the list of stores currently available.
  // We fetch it eagerly so the create/edit dialogs don't show an
  // empty multi-select while waiting.
  const storesQuery = useQuery({
    queryKey: ["admin", "stores"],
    queryFn: fetchStores,
    enabled: isAdmin,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminUserDetailResponse | null>(null);
  const [resetting, setResetting] = useState<AdminUserDetailResponse | null>(
    null,
  );
  const [deleting, setDeleting] = useState<AdminUserDetailResponse | null>(
    null,
  );

  if (!isAdmin) {
    // The router's beforeLoad redirects unauthenticated visitors away
    // from /users; this branch only fires for an authenticated
    // *operator*-role caller who hit /users directly. Show a friendly
    // message rather than a blank table.
    return (
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-(--color-fg)">Users</h2>
        <p
          role="alert"
          className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200"
        >
          Only admin operators can manage users.
        </p>
      </div>
    );
  }

  const availableStores = (storesQuery.data?.items ?? []).map((s) => s.name);
  const users = data?.users ?? [];

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Users</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Operator accounts that can sign in to this admin UI.
            Admins can manage every store; operators are restricted to
            the stores granted here.
          </p>
        </div>
        <Button onClick={() => setCreateOpen(true)}>Create user</Button>
      </header>

      {isLoading ? (
        <p className="text-sm text-(--color-muted)">Loading…</p>
      ) : error ? (
        <p role="alert" className="text-sm text-rose-300">
          {describeApiError(error)}
        </p>
      ) : (
        <UsersTable
          users={users}
          meUsername={meUsername}
          onEdit={(u) => setEditing(u)}
          onReset={(u) => setResetting(u)}
          onDelete={(u) => setDeleting(u)}
        />
      )}

      {createOpen ? (
        <CreateUserDialog
          open={createOpen}
          availableStores={availableStores}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {editing ? (
        <EditUserDialog
          user={editing}
          availableStores={availableStores}
          isLastAdmin={
            editing.role === "admin" &&
            users.filter((u) => u.role === "admin").length <= 1
          }
          onClose={() => setEditing(null)}
        />
      ) : null}
      {resetting ? (
        <ResetPasswordDialog
          user={resetting}
          onClose={() => setResetting(null)}
        />
      ) : null}
      {deleting ? (
        <DeleteUserDialog
          user={deleting}
          isLastAdmin={
            deleting.role === "admin" &&
            users.filter((u) => u.role === "admin").length <= 1
          }
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}

// ---------- Helpers ----------

function RoleBadge({ role }: { role: AdminUserRole }) {
  const isAdmin = role === "admin";
  return (
    <span
      className={
        "inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-xs " +
        (isAdmin
          ? "border-amber-500/30 bg-amber-500/10 text-amber-300"
          : "border-emerald-500/30 bg-emerald-500/10 text-emerald-300")
      }
    >
      {isAdmin ? (
        <ShieldCheck className="h-3 w-3" aria-hidden />
      ) : (
        <UserCog className="h-3 w-3" aria-hidden />
      )}
      {role}
    </span>
  );
}

function formatEpoch(epoch: bigint | number): string {
  // u64 fields come over the wire as bigint; the visible Date range
  // we'd ever show comfortably fits in a JS number.
  const n = typeof epoch === "bigint" ? Number(epoch) : epoch;
  if (!n) return "—";
  return new Date(n * 1000).toLocaleDateString();
}

// ---------- Table ----------

interface UsersTableProps {
  users: AdminUsersListResponse["users"];
  meUsername: string | null;
  onEdit: (u: AdminUserDetailResponse) => void;
  onReset: (u: AdminUserDetailResponse) => void;
  onDelete: (u: AdminUserDetailResponse) => void;
}

function UsersTable({
  users,
  meUsername,
  onEdit,
  onReset,
  onDelete,
}: UsersTableProps) {
  if (users.length === 0) {
    return (
      <p className="text-sm text-(--color-muted)">No users configured.</p>
    );
  }
  return (
    <div className="overflow-x-auto rounded-xl border border-(--color-line)">
      <table className="w-full text-left text-sm">
        <thead className="bg-(--color-surface-0)/60 text-xs uppercase tracking-wide text-(--color-muted)">
          <tr>
            <th className="px-4 py-2">Username</th>
            <th className="px-4 py-2">Role</th>
            <th className="px-4 py-2">Granted stores</th>
            <th className="px-4 py-2">Created</th>
            <th className="px-4 py-2">Password changed</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {users.map((u) => {
            const isMe = meUsername === u.username;
            return (
              <tr
                key={u.id}
                className="border-t border-(--color-line) align-middle"
              >
                <td className="px-4 py-3 font-medium text-(--color-fg)">
                  {u.username}
                  {isMe ? (
                    <span className="ml-2 text-xs text-(--color-muted)">
                      (you)
                    </span>
                  ) : null}
                </td>
                <td className="px-4 py-3">
                  <RoleBadge role={u.role} />
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {u.role === "admin" ? (
                    <span className="text-xs italic">all stores</span>
                  ) : u.granted_stores.length === 0 ? (
                    <span className="text-xs italic">none</span>
                  ) : (
                    <div className="flex flex-wrap gap-1">
                      {u.granted_stores.map((s) => (
                        <span
                          key={s}
                          className="rounded-md border border-(--color-line) bg-(--color-surface-1) px-1.5 py-0.5 text-xs"
                        >
                          {s}
                        </span>
                      ))}
                    </div>
                  )}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {formatEpoch(u.created_at)}
                </td>
                <td className="px-4 py-3 text-(--color-muted)">
                  {formatEpoch(u.password_changed_at)}
                </td>
                <td className="px-4 py-3 text-right">
                  <div className="inline-flex gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onEdit(u)}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onReset(u)}
                    >
                      Reset password
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => onDelete(u)}
                    >
                      Delete
                    </Button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------- Grant editor (shared by create/edit) ----------

interface GrantEditorProps {
  value: string[];
  available: string[];
  disabled?: boolean;
  onChange: (next: string[]) => void;
}

function GrantEditor({ value, available, disabled, onChange }: GrantEditorProps) {
  const set = new Set(value);
  function toggle(name: string) {
    const next = new Set(set);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    onChange([...next].sort());
  }
  if (available.length === 0) {
    return (
      <p className="text-xs text-(--color-muted)">
        No stores configured yet. Create one under Stores first.
      </p>
    );
  }
  return (
    <div className="flex flex-wrap gap-2">
      {available.map((name) => {
        const checked = set.has(name);
        return (
          <label
            key={name}
            className={
              "inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-xs " +
              (checked
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-200"
                : "border-(--color-line) bg-(--color-surface-1) text-(--color-fg)")
            }
          >
            <input
              type="checkbox"
              className="sr-only"
              checked={checked}
              disabled={disabled}
              onChange={() => toggle(name)}
            />
            {name}
          </label>
        );
      })}
    </div>
  );
}

// ---------- Create ----------

const createSchema = z
  .object({
    username: z
      .string()
      .min(1, "Username is required.")
      .regex(
        /^[a-z0-9][a-z0-9_-]*$/,
        "Use lowercase letters, digits, '-' or '_'; must start with a letter or digit.",
      ),
    password: z.string().min(8, "Use at least 8 characters."),
    confirm: z.string(),
    role: z.enum(["admin", "operator"]),
  })
  .refine((v) => v.password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type CreateValues = z.infer<typeof createSchema>;

function CreateUserDialog({
  open,
  availableStores,
  onClose,
}: {
  open: boolean;
  availableStores: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [grants, setGrants] = useState<string[]>([]);
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      username: "",
      password: "",
      confirm: "",
      role: "operator",
    },
  });
  const create = useMutation({
    mutationFn: (body: AdminUserCreateRequest) => postUserCreate(body),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  function onSubmit(values: CreateValues) {
    setSubmitError(null);
    create.mutate({
      username: values.username,
      password: values.password,
      role: values.role,
      // Admins always see everything server-side; we don't send an
      // explicit grant list for admins because the server would just
      // ignore it for authorization purposes.
      granted_stores: values.role === "admin" ? [] : grants,
    });
  }
  const role = form.watch("role");
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create user</DialogTitle>
          <DialogDescription>
            New operators land with the chosen role and an Argon2id
            password hash. The password is sent over TLS, hashed
            server-side, and never echoed back.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="username"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Username</FormLabel>
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
              name="password"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Password</FormLabel>
                  <FormControl>
                    <Input type="password" autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormDescription>
                    At least 8 characters. Longer is better.
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirm"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Confirm password</FormLabel>
                  <FormControl>
                    <Input type="password" autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="role"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Role</FormLabel>
                  <FormControl>
                    <select
                      {...field}
                      className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg)"
                    >
                      <option value="operator">operator</option>
                      <option value="admin">admin</option>
                    </select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {role === "operator" ? (
              <div className="space-y-1.5">
                <Label>Granted stores</Label>
                <GrantEditor
                  value={grants}
                  available={availableStores}
                  onChange={setGrants}
                />
                <p className="text-xs text-(--color-muted)">
                  Operators can only see and act on stores granted here.
                </p>
              </div>
            ) : null}
            {submitError ? (
              <p
                role="alert"
                className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
              >
                {submitError}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Edit ----------

const editSchema = z.object({
  username: z
    .string()
    .min(1, "Username is required.")
    .regex(
      /^[a-z0-9][a-z0-9_-]*$/,
      "Use lowercase letters, digits, '-' or '_'; must start with a letter or digit.",
    ),
  role: z.enum(["admin", "operator"]),
});
type EditValues = z.infer<typeof editSchema>;

function EditUserDialog({
  user,
  availableStores,
  isLastAdmin,
  onClose,
}: {
  user: AdminUserDetailResponse;
  availableStores: string[];
  isLastAdmin: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [grants, setGrants] = useState<string[]>(user.granted_stores ?? []);
  const form = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: { username: user.username, role: user.role },
  });
  const mut = useMutation({
    mutationFn: (body: { username: string; role: AdminUserRole }) =>
      putUserUpdate(user.id, {
        username: body.username,
        role: body.role,
        granted_stores: body.role === "admin" ? [] : grants,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const role = form.watch("role");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit user: {user.username}</DialogTitle>
          <DialogDescription>
            Change username, role, or per-store grants. Use the
            separate “Reset password” action to change a user's
            password.
          </DialogDescription>
        </DialogHeader>
        {isLastAdmin ? (
          <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
            This is the last remaining admin. The role select is
            locked because demoting it would leave the backend
            without an administrator.
          </p>
        ) : null}
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="username"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Username</FormLabel>
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
              name="role"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Role</FormLabel>
                  <FormControl>
                    <select
                      {...field}
                      disabled={isLastAdmin}
                      className="flex h-9 w-full rounded-md border border-(--color-line) bg-(--color-surface-0) px-3 py-2 text-sm text-(--color-fg) disabled:opacity-60"
                    >
                      <option value="operator">operator</option>
                      <option value="admin">admin</option>
                    </select>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {role === "operator" ? (
              <div className="space-y-1.5">
                <Label>Granted stores</Label>
                <GrantEditor
                  value={grants}
                  available={availableStores}
                  onChange={setGrants}
                />
              </div>
            ) : null}
            {submitError ? (
              <p
                role="alert"
                className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
              >
                {submitError}
              </p>
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

// ---------- Reset password ----------

const resetSchema = z
  .object({
    password: z.string().min(8, "Use at least 8 characters."),
    confirm: z.string(),
  })
  .refine((v) => v.password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type ResetValues = z.infer<typeof resetSchema>;

function ResetPasswordDialog({
  user,
  onClose,
}: {
  user: AdminUserDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<ResetValues>({
    resolver: zodResolver(resetSchema),
    defaultValues: { password: "", confirm: "" },
  });
  const mut = useMutation({
    mutationFn: (password: string) =>
      postUserResetPassword(user.id, { password }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Reset password: {user.username}</DialogTitle>
          <DialogDescription>
            Sets a new password for this user. All of the user's
            current sessions are revoked server-side as soon as the
            change lands.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            onSubmit={form.handleSubmit((v) => {
              setSubmitError(null);
              mut.mutate(v.password);
            })}
            className="space-y-4"
            noValidate
          >
            <FormField
              control={form.control}
              name="password"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>New password</FormLabel>
                  <FormControl>
                    <Input type="password" autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirm"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Confirm new password</FormLabel>
                  <FormControl>
                    <Input type="password" autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            {submitError ? (
              <p
                role="alert"
                className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
              >
                {submitError}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" disabled={mut.isPending}>
                {mut.isPending ? "Resetting…" : "Reset password"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// ---------- Delete ----------

function DeleteUserDialog({
  user,
  isLastAdmin,
  onClose,
}: {
  user: AdminUserDetailResponse;
  isLastAdmin: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [confirmText, setConfirmText] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () => deleteUser(user.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const canDelete =
    confirmText === user.username && !mut.isPending && !isLastAdmin;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete user: {user.username}</DialogTitle>
          <DialogDescription>
            This removes the operator account. Any session it holds
            is revoked server-side. Type the username to confirm.
          </DialogDescription>
        </DialogHeader>
        {isLastAdmin ? (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
          >
            This is the last remaining admin. The backend refuses to
            delete it; promote another operator to admin first.
          </p>
        ) : null}
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="confirm-username">
              Type{" "}
              <code className="rounded bg-(--color-surface-1) px-1 py-0.5">
                {user.username}
              </code>{" "}
              to confirm
            </Label>
            <Input
              id="confirm-username"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              autoFocus
              autoComplete="off"
              disabled={isLastAdmin}
            />
          </div>
          {submitError ? (
            <p
              role="alert"
              className="rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {submitError}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            disabled={!canDelete}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Deleting…" : "Delete"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
