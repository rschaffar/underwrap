import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";

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
import {
  type AdminUserCreateRequest,
  type AdminUserDetailResponse,
  type AdminUsersListResponse,
  deleteUser,
  fetchUsers,
  postUserCreate,
  postUserResetPassword,
  putUserUpdate,
  type WhoamiResponse,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

interface UsersPageProps {
  /** Whoami payload from the router context. Used to mark the current
   *  user with "(you)" and to disable self-delete when alone. */
  principal: WhoamiResponse | null;
}

export function UsersPage({ principal }: UsersPageProps) {
  useEffect(() => {
    document.title = "underwrap — users";
  }, []);

  const meUsername = principal?.username ?? null;

  const { data, error, isLoading } = useQuery<AdminUsersListResponse>({
    queryKey: ["admin", "users"],
    queryFn: fetchUsers,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<AdminUserDetailResponse | null>(null);
  const [resetting, setResetting] = useState<AdminUserDetailResponse | null>(
    null,
  );
  const [deleting, setDeleting] = useState<AdminUserDetailResponse | null>(
    null,
  );

  const users = data?.users ?? [];

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-(--color-fg)">Users</h2>
          <p className="mt-1 text-sm text-(--color-muted)">
            Accounts that can sign in to this admin UI. Every user has
            full access — there are no roles or per-store restrictions.
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
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {editing ? (
        <EditUserDialog
          user={editing}
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
          isLastUser={users.length <= 1}
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}

// ---------- Helpers ----------

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
  })
  .refine((v) => v.password === v.confirm, {
    path: ["confirm"],
    message: "Passwords do not match.",
  });
type CreateValues = z.infer<typeof createSchema>;

function CreateUserDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: {
      username: "",
      password: "",
      confirm: "",
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
    });
  }
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create user</DialogTitle>
          <DialogDescription>
            The password is sent over TLS, hashed server-side with
            Argon2id, and never echoed back.
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
});
type EditValues = z.infer<typeof editSchema>;

function EditUserDialog({
  user,
  onClose,
}: {
  user: AdminUserDetailResponse;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: { username: user.username },
  });
  const mut = useMutation({
    mutationFn: (body: { username: string }) =>
      putUserUpdate(user.id, { username: body.username }),
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
          <DialogTitle>Edit user: {user.username}</DialogTitle>
          <DialogDescription>
            Rename the user. Use the separate “Reset password” action
            to change a password.
          </DialogDescription>
        </DialogHeader>
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
  isLastUser,
  onClose,
}: {
  user: AdminUserDetailResponse;
  isLastUser: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const mut = useMutation({
    mutationFn: () => deleteUser(user.id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["admin", "users"] });
      onClose();
    },
    onError: (err) => setSubmitError(describeApiError(err)),
  });
  const canDelete = !mut.isPending && !isLastUser;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete user: {user.username}</DialogTitle>
          <DialogDescription>
            This removes the account. Any session it holds is revoked
            server-side.
          </DialogDescription>
        </DialogHeader>
        {isLastUser ? (
          <p
            role="alert"
            className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200"
          >
            This is the last remaining user. The backend refuses to
            delete it; create another user first.
          </p>
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
