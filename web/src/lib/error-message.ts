import { ApiError } from "@/api/client";

/**
 * Map a backend `ApiError` to a human-readable, *non-sensitive* string
 * suitable for inline display or a toast. The mapping is deliberately
 * coarse: we want operators to recognise the failure category, not to
 * surface server internals or the user's input back to them.
 */
export function describeApiError(err: unknown): string {
  if (err instanceof ApiError) {
    switch (err.code) {
      case "invalid_credentials":
        return "Invalid username or password.";
      case "password_too_short":
        return "Password must be at least 12 characters.";
      case "password_empty":
        return "Password is required.";
      case "rate_limited":
        return "Too many attempts. Please wait a moment and try again.";
      case "bootstrap_already_completed":
        return "First-time setup has already been completed. Sign in instead.";
      case "operators_already_exist":
        return "An operator already exists. Sign in instead.";
      case "csrf_required":
      case "csrf_mismatch":
        return "Your session is stale. Reload the page and try again.";
      case "session_revoked":
        return "Your session was revoked. Sign in again.";
      case "session_key_not_configured":
        return "The backend is misconfigured (no session key). Contact an administrator.";
      default:
        return err.message || "The request failed.";
    }
  }
  return "Network error. Check your connection and try again.";
}
