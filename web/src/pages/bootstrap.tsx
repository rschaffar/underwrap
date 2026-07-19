import { useDocumentTitle } from "@/hooks/use-document-title";

export function BootstrapPage() {
  useDocumentTitle("underwrap — first-time setup");

  return (
    <div className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-lg rounded-xl border border-(--color-line) bg-(--color-surface-0)/70 p-6 shadow-sm">
        <header className="mb-5">
          <h1 className="text-lg font-semibold text-(--color-fg)">
            First-time setup required
          </h1>
          <p className="mt-2 text-sm text-(--color-muted)">
            The first operator must be created on the backend host. Network
            clients cannot create administrator credentials.
          </p>
        </header>

        <div className="space-y-4 text-sm text-(--color-fg)">
          <ol className="list-decimal space-y-2 pl-5">
            <li>Stop the underwrap backend service.</li>
            <li>
              As the backend service identity, run the offline command and
              supply the new password through standard input:
            </li>
          </ol>
          <pre className="overflow-x-auto rounded-md bg-(--color-surface-1) p-3 text-xs">
            <code>
              underwrap-backend --config /path/to/backend.yaml admin
              bootstrap-user --username &lt;name&gt;
            </code>
          </pre>
          <ol start={3} className="list-decimal space-y-2 pl-5">
            <li>Restart the service, then sign in with the new operator.</li>
          </ol>
          <p className="text-(--color-muted)">
            Run the command from a trusted terminal or password-manager flow.
            Never place the password in argv, environment variables,
            configuration, or shell history.
          </p>
          <a
            href="/login"
            className="inline-flex h-9 items-center justify-center rounded-md bg-(--color-accent) px-4 font-medium text-(--color-accent-contrast)"
          >
            Continue to sign in
          </a>
        </div>
      </div>
    </div>
  );
}
