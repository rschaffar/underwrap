export function StatusBadge({ status }: { status: "active" | "disabled" }) {
  const active = status === "active";
  return (
    <span
      className={
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs " +
        (active
          ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
          : "border-amber-500/30 bg-amber-500/10 text-amber-300")
      }
    >
      {status}
    </span>
  );
}

export function AutoCreateBadge({ enabled }: { enabled: boolean }) {
  return (
    <span
      className={
        "inline-flex items-center rounded-md border px-2 py-0.5 text-xs " +
        (enabled
          ? "border-sky-500/30 bg-sky-500/10 text-sky-300"
          : "border-(--color-line) bg-(--color-surface-1) text-(--color-muted)")
      }
      title={
        enabled
          ? "This instance's wrappers may auto-create resource and wrapper/resource-pair records on startup."
          : "This instance's wrappers cannot auto-create resource or pair records. An admin must enable the flag or create them manually."
      }
      data-testid={`auto-create-badge-${enabled ? "on" : "off"}`}
    >
      {enabled ? "auto" : "manual"}
    </span>
  );
}
