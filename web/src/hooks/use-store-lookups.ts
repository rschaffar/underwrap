import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  type AdminClientInstancesListResponse,
  type AdminResourcesListResponse,
  fetchClientInstances,
  postResourcesList,
} from "@/api/client";

/**
 * Shared lookups for per-store admin pages: the resource catalog of a
 * given store and the global client-instance directory. Returns the
 * raw lists alongside `displayNameById` (client-instance id → display
 * name) and `resourceNameById` (resource id → name), since most
 * callers need both shapes.
 */
export function useStoreLookups(storeName: string | null): {
  resources: AdminResourcesListResponse["resources"];
  clientInstances: AdminClientInstancesListResponse["items"];
  displayNameById: Map<string, string>;
  resourceNameById: Map<string, string>;
  isLoading: boolean;
} {
  const resourcesQuery = useQuery<AdminResourcesListResponse>({
    queryKey: ["admin", "resources", storeName],
    queryFn: () => postResourcesList({ store: storeName! }),
    enabled: !!storeName,
  });
  const clientInstancesQuery = useQuery<AdminClientInstancesListResponse>({
    queryKey: ["admin", "client-instances"],
    queryFn: fetchClientInstances,
  });

  const resources = resourcesQuery.data?.resources ?? [];
  const clientInstances = clientInstancesQuery.data?.items ?? [];

  const displayNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of clientInstancesQuery.data?.items ?? []) {
      map.set(i.id, i.display_name);
    }
    return map;
  }, [clientInstancesQuery.data]);

  const resourceNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const r of resourcesQuery.data?.resources ?? []) {
      map.set(r.id, r.name);
    }
    return map;
  }, [resourcesQuery.data]);

  return {
    resources,
    clientInstances,
    displayNameById,
    resourceNameById,
    isLoading: resourcesQuery.isLoading || clientInstancesQuery.isLoading,
  };
}
