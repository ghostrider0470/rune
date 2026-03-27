import { useState } from "react";
import { Link, useLocation } from "@tanstack/react-router";
import {
  LayoutDashboard,
  AlertTriangle,
  MessageSquare,
  MessagesSquare,
  Cpu,
  Clock,
  ShieldCheck,
  Bell,
  Radio,
  Settings,
  Menu,
  BarChart3,
  Bug,
  ScrollText,
  Bot,
  Wrench,
  Settings2,
  BrainCircuit,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ThemeToggle } from "@/components/theme-toggle";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";

interface NavItem {
  label: string;
  href: string;
  icon: React.ComponentType<{ className?: string }>;
  match?: "exact" | "prefix";
}

const navItems: NavItem[] = [
  { label: "Chat", href: "/chat", icon: MessagesSquare, match: "prefix" },
  { label: "Dashboard", href: "/", icon: LayoutDashboard, match: "exact" },
  { label: "Sessions", href: "/sessions", icon: MessageSquare, match: "prefix" },
  { label: "Models", href: "/models", icon: Cpu, match: "prefix" },
  { label: "Cron", href: "/cron", icon: Clock, match: "prefix" },
  { label: "Memory", href: "/memory", icon: BrainCircuit, match: "prefix" },
  { label: "Approvals", href: "/approvals", icon: ShieldCheck, match: "prefix" },
  { label: "Diagnostics", href: "/diagnostics", icon: AlertTriangle, match: "prefix" },
];

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

const moreItems: NavItem[] = [
  { label: "Agents", href: "/agents", icon: Bot, match: "prefix" },
  { label: "Skills", href: "/skills", icon: Wrench, match: "prefix" },
  { label: "Usage", href: "/usage", icon: BarChart3, match: "prefix" },
  { label: "Logs", href: "/logs", icon: ScrollText, match: "prefix" },
  { label: "Config", href: "/config", icon: Settings2, match: "prefix" },
  { label: "Reminders", href: "/reminders", icon: Bell, match: "prefix" },
  { label: "Channels", href: "/channels", icon: Radio, match: "prefix" },
  { label: "Debug", href: "/debug", icon: Bug, match: "prefix" },
  { label: "Settings", href: "/settings", icon: Settings, match: "prefix" },
];

function isActive(pathname: string, item: NavItem): boolean {
  if (item.match === "exact") {
    return pathname === item.href;
  }

  return pathname.startsWith(item.href);
}

export function AdminNavbar() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const location = useLocation();
  const pathname = location.pathname;

  const allItems = [...navItems, ...moreItems];
  const chatLinkSearch = getChatLinkSearch(pathname);

  return (
    <nav
      aria-label="Admin navigation"
      className={cn(
        "sticky top-0 z-[70] w-full border-b bg-background/80 pt-[env(safe-area-inset-top)] backdrop-blur supports-[backdrop-filter]:bg-background/60",
        "transition-all duration-300 shadow-lg shadow-primary/20",
        "border-primary"
      )}
    >
      {/* Subtle gradient accent */}
      <div className="absolute inset-0 bg-gradient-to-r from-primary/5 via-transparent to-accent/5 pointer-events-none z-0" />

      <div className="relative z-10 mx-auto w-full max-w-7xl px-3 sm:px-6 lg:px-8">
        <div className="flex min-h-16 items-center justify-between gap-3 py-2 sm:min-h-[4.5rem]">
          {/* Left: Logo + Admin badge + nav links */}
          <div className="flex min-w-0 items-center gap-3 sm:gap-4">
            <Link
              to="/chat"
              search={chatLinkSearch}
              className="flex min-w-0 items-center gap-2 rounded-md px-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <img
                src="/assets/rune-logo-icon.svg"
                alt="Rune"
                className="h-10 w-auto py-0.5 sm:h-12"
                decoding="async"
              />
              <Badge
                variant="outline"
                className="hidden sm:inline-flex shrink-0 border-primary/30 bg-primary/10 text-primary text-[10px]"
              >
                Chat-first Admin
              </Badge>
            </Link>

            {/* Desktop nav links */}
            <div className="hidden lg:flex items-center gap-1.5">
              {navItems.map((item) => {
                const active = isActive(pathname, item);
                const Icon = item.icon;
                return (
                  <Link
                    key={item.href}
                    to={item.href}
                    search={item.href === "/chat" ? chatLinkSearch : undefined}
                    className={cn(
                      "flex items-center gap-2 rounded-lg px-3.5 py-2.5 text-sm font-medium transition-colors",
                      active
                        ? "bg-primary/10 text-primary"
                        : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                    )}
                  >
                    <Icon className="size-4 shrink-0" />
                    <span>{item.label}</span>
                  </Link>
                );
              })}
              {moreItems.map((item) => {
                const active = isActive(pathname, item);
                const Icon = item.icon;
                return (
                  <Link
                    key={item.href}
                    to={item.href}
                    search={item.href === "/chat" ? chatLinkSearch : undefined}
                    className={cn(
                      "flex items-center gap-2 rounded-lg px-3.5 py-2.5 text-sm font-medium transition-colors",
                      active
                        ? "bg-primary/10 text-primary"
                        : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                    )}
                  >
                    <Icon className="size-4 shrink-0" />
                    <span>{item.label}</span>
                  </Link>
                );
              })}
            </div>
          </div>

          {/* Right: actions */}
          <div className="flex shrink-0 items-center gap-2">
            <ThemeToggle />

            {/* Mobile menu trigger */}
            <Sheet open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
              <SheetTrigger asChild className="lg:hidden">
                <Button variant="ghost" size="icon" className="h-11 w-11">
                  <Menu className="h-5 w-5" />
                  <span className="sr-only">Open admin menu</span>
                </Button>
              </SheetTrigger>
              <SheetContent
                side="right"
                className="inset-y-auto top-[calc(3.5rem+env(safe-area-inset-top))] h-[calc(100dvh-3.5rem-env(safe-area-inset-top))] w-screen max-w-none border-l-0 bg-background p-0 [&>button]:hidden sm:top-[calc(4rem+env(safe-area-inset-top))] sm:h-[calc(100dvh-4rem-env(safe-area-inset-top))] sm:max-w-none"
              >
                <SheetHeader className="border-b px-6 pb-5 pt-5">
                  <SheetTitle className="px-0 pt-0">
                    <div className="text-left">
                      <p className="text-xs text-muted-foreground uppercase tracking-wider">
                        Administration
                      </p>
                      <p className="text-base font-semibold leading-tight">
                        Rune Admin Panel
                      </p>
                    </div>
                  </SheetTitle>
                </SheetHeader>

                <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 py-5 sm:px-6 sm:py-6">
                  <div className="mx-auto grid w-full max-w-md gap-5 pb-[max(0.5rem,env(safe-area-inset-bottom))]">
                    <div className="grid gap-3">
                      {allItems.map((item) => {
                        const active = isActive(pathname, item);
                        const Icon = item.icon;
                        return (
                          <Link
                            key={item.href}
                            to={item.href}
                            search={item.href === "/chat" ? chatLinkSearch : undefined}
                            onClick={() => setMobileMenuOpen(false)}
                            className={cn(
                              "flex min-h-12 items-center gap-3 rounded-xl border px-4 py-3 font-medium transition-all active:scale-[0.98]",
                              active
                                ? "border-primary/40 bg-primary/10 text-primary"
                                : "border-border bg-card/70 hover:border-primary/40 hover:bg-primary/10"
                            )}
                          >
                            <Icon className="size-5 shrink-0" />
                            <span>{item.label}</span>
                          </Link>
                        );
                      })}
                    </div>
                  </div>
                </div>
              </SheetContent>
            </Sheet>
          </div>
        </div>
      </div>
    </nav>
  );
}
