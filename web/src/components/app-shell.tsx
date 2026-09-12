import { useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import {
  Menu,
  Moon,
  Sun,
  LayoutDashboard,
  Users,
  Lock,
  LogOut,
  UserCircle,
  Grid3x3,
  KeyRound,
  Monitor,
  Rocket,
} from "lucide-react";

import type { AdminStoreListItem } from "@/api/client";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { useTheme } from "@/hooks/use-theme";

interface NavItem {
  label: string;
  icon: typeof LayoutDashboard;
  href: Parameters<typeof Link>[0]["to"];
}

const NAV = [
  { label: "Get a wrapper working", icon: Rocket, href: "/setup" },
  { label: "Wrapper permissions", icon: Grid3x3, href: "/wrappers" },
  { label: "Secrets", icon: KeyRound, href: "/secrets" },
  { label: "Stores", icon: Lock, href: "/stores" },
  { label: "Client Instances", icon: Monitor, href: "/client-instances" },
  { label: "Users", icon: Users, href: "/users" },
  { label: "Profile", icon: UserCircle, href: "/profile" },
] as const satisfies readonly NavItem[];

function Sidebar({
  storeId,
  preserveRequested,
  onNavigate,
}: {
  storeId: string | null;
  preserveRequested: boolean;
  onNavigate?: () => void;
}) {
  return (
    <nav aria-label="Primary" className="flex h-full flex-col gap-1 p-3">
      <div className="px-3 py-4 text-sm font-semibold tracking-wide text-(--color-muted) uppercase">
        underwrap
      </div>
      {NAV.map(({ label, icon: Icon, href }) => (
        <Link
          key={label}
          to={href}
          // While metadata is unresolved, retain requested state for validation
          // at the destination; do not silently replace it with the first store.
          search={(previous) => {
            const requested = Array.isArray(previous.store)
              ? previous.store[0]
              : previous.store;
            const next =
              storeId ??
              (preserveRequested && typeof requested === "string"
                ? requested
                : undefined);
            return next ? { store: next } : {};
          }}
          activeOptions={{ exact: true, includeSearch: false }}
          onClick={onNavigate}
          className="flex items-center gap-3 rounded-md px-3 py-2 text-sm text-(--color-fg)/85 hover:bg-(--color-surface-1) hover:text-(--color-fg) aria-[current=page]:bg-(--color-surface-1) aria-[current=page]:font-semibold aria-[current=page]:text-(--color-fg)"
        >
          <Icon className="h-4 w-4 shrink-0" aria-hidden />
          {label}
        </Link>
      ))}
    </nav>
  );
}

interface AppShellProps {
  children: ReactNode;
  storeId?: string | null;
  store?: AdminStoreListItem;
  storeStatus?: "pending" | "error" | "resolved";
}

/** Presentational chrome: data acquisition belongs to StoreNavigation. */
export function AppShell({
  children,
  storeId = null,
  store,
  storeStatus = "pending",
}: AppShellProps) {
  const { theme, toggle } = useTheme();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const selectedStore =
    storeStatus === "resolved" && store?.id === storeId ? store : undefined;
  const selectedStoreId = selectedStore?.id ?? null;

  return (
    <div className="min-h-screen text-(--color-fg)">
      <header className="sticky top-0 z-30 grid min-h-14 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 border-b border-(--color-line) bg-(--color-surface-0)/80 px-4 py-2 backdrop-blur md:h-14 md:grid-cols-[auto_minmax(0,1fr)_auto] md:py-0">
        <Sheet open={drawerOpen} onOpenChange={setDrawerOpen}>
          <SheetTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              aria-label="Open navigation"
            >
              <Menu className="h-5 w-5" />
            </Button>
          </SheetTrigger>
          <SheetContent side="left" className="p-0">
            <SheetHeader className="p-4 pb-0">
              <SheetTitle>Navigation</SheetTitle>
              <SheetDescription className="sr-only">
                Top-level sections of the underwrap admin UI.
              </SheetDescription>
            </SheetHeader>
            <Sidebar
              storeId={selectedStoreId}
              preserveRequested={storeStatus !== "resolved"}
              onNavigate={() => setDrawerOpen(false)}
            />
          </SheetContent>
        </Sheet>

        <h1 className="min-w-0 text-base font-semibold tracking-wide">
          underwrap
        </h1>

        <div
          role="status"
          aria-label="Selected store context"
          className="col-span-3 row-start-2 min-w-0 text-xs md:col-span-1 md:col-start-2 md:row-start-1"
        >
          {selectedStore ? (
            <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 md:flex-nowrap">
              <span
                className="min-w-0 truncate font-medium"
                title={selectedStore.name}
              >
                Store: {selectedStore.name}
              </span>
              <span className="shrink-0 text-(--color-muted)">
                {selectedStore.backend === "encrypted" ? "Encrypted" : "Plain"}
                {" · "}
                {selectedStore.lifecycle_state}
              </span>
            </div>
          ) : (
            <span className="text-(--color-muted)">
              {storeStatus === "error"
                ? "Store context could not be loaded"
                : storeStatus === "pending"
                  ? "Loading store context…"
                  : "No store selected"}
            </span>
          )}
        </div>

        <div className="col-start-3 row-start-1 flex shrink-0 items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            aria-label={
              theme === "dark"
                ? "Switch to light theme"
                : "Switch to dark theme"
            }
            onClick={toggle}
          >
            {theme === "dark" ? (
              <Sun className="h-4 w-4" />
            ) : (
              <Moon className="h-4 w-4" />
            )}
          </Button>
          <Button variant="ghost" size="icon" aria-label="Sign out" asChild>
            <Link to="/logout">
              <LogOut className="h-4 w-4" />
            </Link>
          </Button>
        </div>
      </header>

      <div className="flex">
        <aside
          aria-label="Sidebar"
          className="sticky top-14 hidden h-[calc(100vh-3.5rem)] w-60 shrink-0 border-r border-(--color-line) bg-(--color-surface-0)/50 md:block"
        >
          <Sidebar
            storeId={selectedStoreId}
            preserveRequested={storeStatus !== "resolved"}
          />
        </aside>
        <main className="min-w-0 flex-1 p-4 md:p-6">{children}</main>
      </div>
    </div>
  );
}
