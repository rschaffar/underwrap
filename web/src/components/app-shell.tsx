import { useState, type ReactNode } from "react";
import { Menu, Moon, Sun, Box, Users, Lock, ShieldCheck, History } from "lucide-react";

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
}

const NAV: NavItem[] = [
  { label: "Resources", icon: Box, href: "/" },
  { label: "Wrappers", icon: ShieldCheck, href: "/wrappers" },
  { label: "Stores", icon: Lock, href: "/stores" },
  { label: "Operators", icon: Users, href: "/operators" },
  { label: "Audit", icon: History, href: "/audit" },
];

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <nav aria-label="Primary" className="flex h-full flex-col gap-1 p-3">
      <div className="px-3 py-4 text-sm font-semibold tracking-wide text-(--color-muted) uppercase">
        underwrap
      </div>
      {NAV.map(({ label, icon: Icon, href }) => (
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

export function AppShell({ children }: { children: ReactNode }) {
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
            <Sidebar onNavigate={() => setDrawerOpen(false)} />
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
        </div>
      </header>

      <div className="flex">
        <aside
          aria-label="Sidebar"
          className="sticky top-14 hidden h-[calc(100vh-3.5rem)] w-60 shrink-0 border-r border-(--color-line) bg-(--color-surface-0)/50 md:block"
        >
          <Sidebar />
        </aside>
        <main className="min-w-0 flex-1 p-6">{children}</main>
      </div>
    </div>
  );
}
