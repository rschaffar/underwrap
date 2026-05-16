import { useState, type ReactNode } from "react";
import {
  Menu,
  Moon,
  Sun,
  Box,
  Users,
  Lock,
  ShieldCheck,
  History,
  LogOut,
  UserCircle,
} from "lucide-react";

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
import { cn } from "@/lib/utils";

interface NavItem {
  label: string;
  icon: typeof Box;
  href: string;
  /** Hide this nav item unless the role string is admin. */
  adminOnly?: boolean;
}

const NAV: NavItem[] = [
  { label: "Resources", icon: Box, href: "/" },
  { label: "Wrappers", icon: ShieldCheck, href: "/wrappers" },
  { label: "Stores", icon: Lock, href: "/stores", adminOnly: true },
  { label: "Users", icon: Users, href: "/users", adminOnly: true },
  { label: "Audit", icon: History, href: "/audit" },
  { label: "Profile", icon: UserCircle, href: "/profile" },
];

function Sidebar({
  onNavigate,
  role,
}: {
  onNavigate?: () => void;
  role?: string;
}) {
  return (
    <nav aria-label="Primary" className="flex h-full flex-col gap-1 p-3">
      <div className="px-3 py-4 text-sm font-semibold tracking-wide text-(--color-muted) uppercase">
        underwrap
      </div>
      {NAV.filter((n) => !n.adminOnly || role === "admin").map(({ label, icon: Icon, href }) => (
        <a
          key={label}
          href={href}
          onClick={onNavigate}
          className="flex items-center gap-3 rounded-md px-3 py-2 text-sm text-(--color-fg)/85 hover:bg-(--color-surface-1) hover:text-(--color-fg)"
        >
          <Icon className="h-4 w-4" aria-hidden />
          {label}
        </a>
      ))}
    </nav>
  );
}

interface AppShellProps {
  children: ReactNode;
  /** Optional sign-out callback. When omitted the logout button is hidden. */
  onLogout?: () => void | Promise<void>;
  /**
   * Role string from `whoami`. Drives admin-only nav-item visibility.
   * Omit for the unauthenticated case (no nav items rendered for
   * admin-only entries).
   */
  role?: string;
}

export function AppShell({ children, onLogout, role }: AppShellProps) {
  const { theme, toggle } = useTheme();
  const [drawerOpen, setDrawerOpen] = useState(false);

  return (
    <div className="min-h-screen text-(--color-fg)">
      <header
        className={cn(
          "sticky top-0 z-30 flex h-14 items-center gap-3 border-b border-(--color-line) bg-(--color-surface-0)/80 px-4 backdrop-blur",
        )}
      >
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
              onNavigate={() => setDrawerOpen(false)}
              role={role}
            />
          </SheetContent>
        </Sheet>

        <h1 className="text-base font-semibold tracking-wide">underwrap</h1>

        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            onClick={toggle}
          >
            {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </Button>
          {onLogout ? (
            <Button
              variant="ghost"
              size="icon"
              aria-label="Sign out"
              onClick={() => {
                void onLogout();
              }}
            >
              <LogOut className="h-4 w-4" />
            </Button>
          ) : null}
        </div>
      </header>

      <div className="flex">
        <aside
          aria-label="Sidebar"
          className="sticky top-14 hidden h-[calc(100vh-3.5rem)] w-60 shrink-0 border-r border-(--color-line) bg-(--color-surface-0)/50 md:block"
        >
          <Sidebar role={role} />
        </aside>
        <main className="min-w-0 flex-1 p-6">{children}</main>
      </div>
    </div>
  );
}
