import { type ReactNode, useMemo, useState } from "react";
import { useQuery, type QueryKey } from "@tanstack/react-query";

import { AdminDialog } from "@/components/admin-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { describeApiError } from "@/lib/error-message";

/**
 * Self-contained "reveal a secret value" dialog. Owns the `AdminDialog`
 * shell *and* the body so flat-KV and libsecret callers stop repeating
 * the same Radix boilerplate around it.
 *
 * UX: confirm gate → fetch → b64 decode → textarea + copy-to-clipboard.
 * The differences between the two use sites are exactly the query
 * key, the fetch function, the title/description, and the textarea
 * id.
 */
export function RevealSecretView<T>({
  title,
  description,
  queryKey,
  queryFn,
  pickB64,
  textareaId,
  textareaTestId,
  onClose,
}: {
  title: ReactNode;
  description: ReactNode;
  queryKey: QueryKey;
  queryFn: () => Promise<T>;
  /**
   * Extract the base64 secret payload from the API response. Return
   * `null` / `undefined` if the response indicates the value is
   * absent (e.g. flat-KV `material_b64: null`).
   */
  pickB64: (response: T) => string | null | undefined;
  textareaId: string;
  textareaTestId: string;
  onClose: () => void;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const [copied, setCopied] = useState(false);
  const read = useQuery<T>({
    queryKey,
    queryFn,
    enabled: confirmed,
    // Don't cache the secret material in the React Query store any
    // longer than necessary: gcTime: 0 collects the entry as soon as
    // this dialog unmounts. There is no explicit zero-fill — the value
    // lives in the JS heap (query result + cache object) until the next
    // GC cycle. Acceptable for a browser admin SPA; documented so the
    // boundary is clear to future maintainers.
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: "always",
  });

  const b64 = read.data ? pickB64(read.data) : null;
  const decoded = useMemo(() => {
    if (!b64) return null;
    try {
      return atob(b64);
    } catch {
      return null;
    }
  }, [b64]);

  async function onCopy() {
    if (!decoded) return;
    try {
      await navigator.clipboard.writeText(decoded);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Non-secure-context fallback: do nothing; the textarea is
      // selectable for manual copy.
    }
  }

  let content: ReactNode;
  if (!confirmed) {
    content = (
      <>
        <p className="text-xs text-(--color-muted)">
          Press <strong>Show</strong> to fetch the value. It will be
          displayed in plain text in this dialog.
        </p>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button type="button" onClick={() => setConfirmed(true)}>
            Show
          </Button>
        </DialogFooter>
      </>
    );
  } else {
    let body: ReactNode;
    if (read.isLoading) {
      body = <p className="text-sm text-(--color-muted)">Loading…</p>;
    } else if (read.error) {
      body = <Alert>{describeApiError(read.error)}</Alert>;
    } else if (decoded == null) {
      body = (
        <Alert variant="warning">
          {b64 == null
            ? "The secret does not exist (or was removed)."
            : "The server returned a value that could not be base64-decoded. This is likely a server-side bug."}
        </Alert>
      );
    } else {
      body = (
        <div className="space-y-2">
          <Label htmlFor={textareaId}>Value</Label>
          <textarea
            id={textareaId}
            data-testid={textareaTestId}
            readOnly
            value={decoded}
            rows={6}
            className="w-full rounded-md border border-(--color-line) bg-(--color-surface-0) p-2 font-mono text-xs text-(--color-fg)"
          />
          <Button type="button" variant="ghost" size="sm" onClick={onCopy}>
            {copied ? "Copied" : "Copy to clipboard"}
          </Button>
        </div>
      );
    }
    content = (
      <>
        {body}
        <DialogFooter>
          <Button type="button" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </>
    );
  }
  return (
    <AdminDialog open onClose={onClose} title={title} description={description}>
      {content}
    </AdminDialog>
  );
}
