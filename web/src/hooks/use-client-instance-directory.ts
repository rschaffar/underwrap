import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  type AdminClientInstancesListResponse,
  fetchClientInstances,
} from "@/api/client";

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
  isSuccess: boolean;
} {
  const query = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });

  const clientInstances = useMemo(() => query.data?.items ?? [], [query.data]);

  const displayNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of clientInstances) {
      map.set(i.id, i.display_name);
    }
    return map;
  }, [clientInstances]);

  return {
    clientInstances,
    displayNameById,
    isLoading: query.isLoading,
    isSuccess: query.isSuccess,
  };
}
