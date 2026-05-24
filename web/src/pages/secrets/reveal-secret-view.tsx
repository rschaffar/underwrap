import { type ReactNode, useMemo, useState } from "react";
import { useQuery, type QueryKey } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { describeApiError } from "@/lib/error-message";

/**
 * Shared body for the "reveal a secret value" dialogs.
 *
 * Both the flat-KV secret reveal and the libsecret item-secret reveal
 * follow the same UX: confirm gate → fetch → b64 decode → textarea +
 * copy-to-clipboard. The differences between the two are exactly the
 * query key, the fetch function, and the textarea id; everything else
 * is shared here.
 *
 * The body owns the dialog's main content area and the trailing
 * footer. The caller is responsible for rendering the dialog shell
 * (title, description) and the Close handler.
 */
export function RevealSecretView<T>({
  queryKey,
  queryFn,
  pickB64,
  textareaId,
  textareaTestId,
  onClose,
}: {
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
    // longer than necessary.
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
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Non-secure-context fallback: do nothing; the textarea is
      // selectable for manual copy.
    }
  }

  if (!confirmed) {
    return (
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
  }

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
          : "The secret material could not be decoded as UTF-8 text."}
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

  return (
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
