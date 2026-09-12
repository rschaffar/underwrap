import { useEffect } from "react";
import { useRouter } from "@tanstack/react-router";

/** Prevent incidental route loss; browser unload prompts remain best effort. */
export function useNavigationLock(active: boolean) {
  const router = useRouter({ warn: false });
  useEffect(() => {
    if (!active) return;
    const unblock = router?.history.block({
      blockerFn: () => true,
      enableBeforeUnload: false,
    });
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", beforeUnload);
    return () => {
      unblock?.();
      window.removeEventListener("beforeunload", beforeUnload);
    };
  }, [active, router]);
}
