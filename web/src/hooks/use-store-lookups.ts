import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import { type AdminClientInstancesListResponse, fetchClientInstances } from "@/api/client";

/**
 * Shared lookups for per-store admin pages: the global
 * client-instance directory plus a `displayNameById` map for table
 * rendering. Per-store resource catalogs travel inside the
 * aggregate-view endpoints (`wrapper-permissions-view`,
 * `secrets-view`) — no separate per-store resources query lives
 * here.
 */
export function useClientInstanceDirectory(): {
  clientInstances: AdminClientInstancesListResponse["items"];
  displayNameById: Map<string, string>;
  isLoading: boolean;
} {
  const query = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });

  const clientInstances = query.data?.items ?? [];

  const displayNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of query.data?.items ?? []) {
      map.set(i.id, i.display_name);
    }
    return map;
  }, [query.data]);

  return {
    clientInstances,
    displayNameById,
    isLoading: query.isLoading,
  };
}
