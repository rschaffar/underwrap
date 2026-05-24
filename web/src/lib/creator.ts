import type { EntityCreator } from "@/api/client";

/**
 * Render an `EntityCreator` as a short human-readable label. When the
 * creator is a runtime client instance, prefer the operator-facing
 * display name resolved from `displayNameById` over the opaque id.
 *
 * Returns just the name. Callers that need to distinguish operators
 * from client instances should add their own context (e.g. a kind
 * badge or a prefixed string).
 */
export function describeCreator(
  creator: EntityCreator,
  displayNameById?: Map<string, string>,
): string {
  switch (creator.kind) {
    case "operator":
      return creator.username;
    case "client-instance":
      return displayNameById?.get(creator.id) ?? creator.id;
  }
}
