import { Link, useLocation } from "@tanstack/react-router";
import {
  LayoutDashboard,
  MessageSquare,
  MessagesSquare,
  Clock,
  Settings,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { designSystem } from "@/lib/design-system";
import type { ReactNode } from "react";

type NavItem = {
  icon: ReactNode;
  label: string;
  href: string;
  match?: "exact" | "prefix";
};

function getChatLinkSearch(pathname: string): { session: string | undefined } {
  const fallback = { session: undefined };
  const chatPrefix = "/chat";
  if (!pathname.startsWith(chatPrefix)) {
    return fallback;
  }

  const search = typeof window !== "undefined" ? window.location.search : "";
  const params = new URLSearchParams(search);
  const session = params.get("session");

  return { session: session ?? undefined };
}

const navItems: NavItem[] = [
  {
    icon: <MessagesSquare className="h-6 w-6" />,
    label: "Chat",
    href: "/chat",
    match: "prefix",
  },
  {
    icon: <LayoutDashboard className="h-6 w-6" />,
    label: "Dashboard",
    href: "/",
    match: "exact",
  },
  {
    icon: <MessageSquare className="h-6 w-6" />,
    label: "Sessions",
    href: "/sessions",
    match: "prefix",
  },
  {
    icon: <Clock className="h-6 w-6" />,
    label: "Cron",
    href: "/cron",
    match: "prefix",
  },
  {
    icon: <Settings className="h-6 w-6" />,
    label: "Settings",
    href: "/settings",
    match: "prefix",
  },
];

export function AdminBottomNav() {
  const location = useLocation();
  const pathname = location.pathname;

  const chatLinkSearch = getChatLinkSearch(pathname);

  const isActive = (item: NavItem) => {
    if (item.match === "exact") {
      return pathname === item.href;
    }
    return pathname.startsWith(item.href);
  };

  return (
    <nav
      aria-label="Admin navigation"
      className="fixed bottom-0 left-0 right-0 z-40 pl-[max(0.75rem,env(safe-area-inset-left))] pr-[max(0.75rem,env(safe-area-inset-right))] pb-[max(0.75rem,env(safe-area-inset-bottom))] pt-2 lg:hidden"
    >
      <div className="mx-auto w-full max-w-lg rounded-2xl border border-primary/20 bg-background/92 shadow-[0_10px_35px_rgba(15,23,42,0.18)] backdrop-blur supports-[backdrop-filter]:bg-background/80">
        <div className="flex items-center justify-between px-4 py-3">
          {navItems.map((item) => {
            const active = isActive(item);
            return (
              <Link
                key={item.href}
                to={item.href}
                search={item.href === "/chat" ? chatLinkSearch : undefined}
                aria-label={item.label}
                className={cn(
                  "relative flex min-h-14 min-w-[48px] flex-1 flex-col items-center justify-center gap-2 rounded-xl text-xs font-medium transition-all duration-200",
                  designSystem.effects.focusRing,
                  active
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:text-foreground"
                )}
              >
                <div
                  className={cn(
                    "transition-transform duration-200",
                    active && "scale-110"
                  )}
                >
                  {item.icon}
                </div>
                <span className="max-w-[60px] truncate text-[11px] leading-none">
                  {item.label}
                </span>
                {active && (
                  <div className="absolute inset-x-3 bottom-0 h-0.5 rounded-full bg-gradient-to-r from-primary to-accent" />
                )}
              </Link>
            );
          })}
        </div>
      </div>
    </nav>
  );
}
