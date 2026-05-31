import { StatusChip } from "@/components/ui/status-chip";

export function StatusBadge({ status }: { status: "active" | "disabled" }) {
  return (
    <StatusChip tone={status === "active" ? "emerald" : "amber"}>
      {status}
    </StatusChip>
  );
}

export function AutoCreateBadge({ enabled }: { enabled: boolean }) {
  return (
    <StatusChip
      tone={enabled ? "sky" : "neutral"}
      title={
        enabled
          ? "This instance's wrappers may auto-create resource and wrapper/resource-pair records on startup."
          : "This instance's wrappers cannot auto-create resource or pair records. An admin must enable the flag or create them manually."
      }
      data-testid={`auto-create-badge-${enabled ? "on" : "off"}`}
    >
      {enabled ? "auto" : "manual"}
    </StatusChip>
  );
}
