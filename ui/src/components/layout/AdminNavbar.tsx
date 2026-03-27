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
import { ThemeToggle } from "@/components/theme-toggle";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
  navigationMenuTriggerStyle,
} from "@/components/ui/navigation-menu";
import { cn } from "@/lib/utils";

interface NavItem {
  label: string;
  href: string;
  icon: React.ComponentType<{ className?: string }>;
  description: string;
  match?: "exact" | "prefix";
}

const primaryItems: NavItem[] = [
  { label: "Chat", href: "/chat", icon: MessagesSquare, description: "Conversational interface", match: "prefix" },
  { label: "Dashboard", href: "/", icon: LayoutDashboard, description: "System overview", match: "exact" },
  { label: "Sessions", href: "/sessions", icon: MessageSquare, description: "Active conversations", match: "prefix" },
];

const opsItems: NavItem[] = [
  { label: "Models", href: "/models", icon: Cpu, description: "Provider routing and config", match: "prefix" },
  { label: "Cron", href: "/cron", icon: Clock, description: "Scheduled jobs", match: "prefix" },
  { label: "Memory", href: "/memory", icon: BrainCircuit, description: "Knowledge graph", match: "prefix" },
  { label: "Approvals", href: "/approvals", icon: ShieldCheck, description: "Pending tool authorizations", match: "prefix" },
  { label: "Diagnostics", href: "/diagnostics", icon: AlertTriangle, description: "Health checks and alerts", match: "prefix" },
];

const adminItems: NavItem[] = [
  { label: "Agents", href: "/agents", icon: Bot, description: "Agent registry", match: "prefix" },
  { label: "Skills", href: "/skills", icon: Wrench, description: "Skill plugins", match: "prefix" },
  { label: "Usage", href: "/usage", icon: BarChart3, description: "Token usage and costs", match: "prefix" },
  { label: "Logs", href: "/logs", icon: ScrollText, description: "System log viewer", match: "prefix" },
  { label: "Config", href: "/config", icon: Settings2, description: "Runtime configuration", match: "prefix" },
  { label: "Reminders", href: "/reminders", icon: Bell, description: "Scheduled notifications", match: "prefix" },
  { label: "Channels", href: "/channels", icon: Radio, description: "Messaging integrations", match: "prefix" },
  { label: "Debug", href: "/debug", icon: Bug, description: "Developer tools", match: "prefix" },
  { label: "Settings", href: "/settings", icon: Settings, description: "Global preferences", match: "prefix" },
];

const allItems: NavItem[] = [...primaryItems, ...opsItems, ...adminItems];

function getChatLinkSearch(pathname: string): { session: string | undefined } {
  if (!pathname.startsWith("/chat")) return { session: undefined };
  const params = new URLSearchParams(
    typeof window !== "undefined" ? window.location.search : ""
  );
  return { session: params.get("session") ?? undefined };
}

function isActive(pathname: string, item: NavItem): boolean {
  return item.match === "exact"
    ? pathname === item.href
    : pathname.startsWith(item.href);
}

function NavDropdownItem({
  item,
  pathname,
  chatLinkSearch,
}: {
  item: NavItem;
  pathname: string;
  chatLinkSearch: { session: string | undefined };
}) {
  const Icon = item.icon;
  const active = isActive(pathname, item);
  return (
    <li>
      <Link to={item.href} search={item.href === "/chat" ? chatLinkSearch : undefined}>
        <NavigationMenuLink asChild>
          <div
            className={cn(
              "block select-none space-y-1 rounded-md p-3 leading-none no-underline outline-none group",
              "transition-all hover:bg-primary/10 focus:bg-primary/10",
              "hover:shadow-sm border border-transparent hover:border-primary/30",
              active && "bg-primary/10 border-primary/40"
            )}
          >
            <div className="flex items-center gap-2 text-sm font-medium leading-none group-hover:text-foreground">
              <Icon className="size-4 shrink-0 text-muted-foreground group-hover:text-primary" />
              {item.label}
            </div>
            <p className="line-clamp-1 text-xs leading-snug text-muted-foreground group-hover:text-foreground/70">
              {item.description}
            </p>
          </div>
        </NavigationMenuLink>
      </Link>
    </li>
  );
}

export function AdminNavbar() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const location = useLocation();
  const pathname = location.pathname;
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
      <div className="absolute inset-0 bg-gradient-to-r from-primary/5 via-transparent to-accent/5 pointer-events-none z-0" />

      <div className="relative z-10 mx-auto w-full max-w-7xl px-3 sm:px-6 lg:px-8">
        <div className="flex h-16 items-center justify-between">
          {/* Left: Logo + nav */}
          <div className="flex items-center gap-3 md:gap-5">
            <Link
              to="/chat"
              search={chatLinkSearch}
              className="flex shrink-0 items-center gap-2.5 rounded-md px-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <svg
                viewBox="0 0 240 320"
                className="h-8 w-auto sm:h-9"
                aria-label="Rune"
              >
                <path
                  fill="currentColor"
                  fillRule="evenodd"
                  d="M105,26 L119,40 L119,129 L157,91 L157,111 L119,149 L119,173 L172,226 L105,293 L105,179 L67,141 L67,121 L105,159 L105,119 L67,81 L67,61 L105,99 Z M119,193 L119,259 L152,226 Z"
                />
              </svg>
              <span className="hidden sm:block text-base font-semibold leading-tight tracking-tight">
                Rune
              </span>
            </Link>

            {/* Desktop Navigation */}
            <div className="hidden lg:block h-6 w-px bg-border/60 mx-1" />
            <NavigationMenu viewport={false} className="hidden lg:flex">
              <NavigationMenuList>
                {/* Primary direct links */}
                {primaryItems.map((item) => {
                  const Icon = item.icon;
                  const active = isActive(pathname, item);
                  return (
                    <NavigationMenuItem key={item.href}>
                      <NavigationMenuLink asChild>
                        <Link
                          to={item.href}
                          search={item.href === "/chat" ? chatLinkSearch : undefined}
                          className={cn(
                            navigationMenuTriggerStyle(),
                            "gap-2 bg-transparent",
                            active
                              ? "text-primary bg-primary/10"
                              : "text-muted-foreground"
                          )}
                        >
                          <Icon className="size-4 shrink-0" />
                          {item.label}
                        </Link>
                      </NavigationMenuLink>
                    </NavigationMenuItem>
                  );
                })}

                {/* Operations dropdown */}
                <NavigationMenuItem>
                  <NavigationMenuTrigger className="bg-transparent text-muted-foreground transition-colors data-[state=open]:bg-accent/60 data-[state=open]:text-accent-foreground">
                    Operations
                  </NavigationMenuTrigger>
                  <NavigationMenuContent className="shadow-xl">
                    <ul className="grid w-[420px] gap-2 p-3 md:grid-cols-2">
                      {opsItems.map((item) => (
                        <NavDropdownItem
                          key={item.href}
                          item={item}
                          pathname={pathname}
                          chatLinkSearch={chatLinkSearch}
                        />
                      ))}
                    </ul>
                  </NavigationMenuContent>
                </NavigationMenuItem>

                {/* Admin dropdown */}
                <NavigationMenuItem>
                  <NavigationMenuTrigger className="bg-transparent text-muted-foreground transition-colors data-[state=open]:bg-accent/60 data-[state=open]:text-accent-foreground">
                    Admin
                  </NavigationMenuTrigger>
                  <NavigationMenuContent className="shadow-xl">
                    <ul className="grid w-[500px] gap-2 p-3 md:grid-cols-3">
                      {adminItems.map((item) => (
                        <NavDropdownItem
                          key={item.href}
                          item={item}
                          pathname={pathname}
                          chatLinkSearch={chatLinkSearch}
                        />
                      ))}
                    </ul>
                  </NavigationMenuContent>
                </NavigationMenuItem>
              </NavigationMenuList>
            </NavigationMenu>
          </div>

          {/* Right: actions */}
          <div className="flex shrink-0 items-center gap-1 sm:gap-2">
            <ThemeToggle />

            {/* Mobile menu */}
            <Sheet open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
              <SheetTrigger asChild className="lg:hidden">
                <Button variant="ghost" size="icon" className="h-9 w-9">
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
