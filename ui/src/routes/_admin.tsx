import { createFileRoute, Outlet } from "@tanstack/react-router";
import { AdminNavbar } from "@/components/layout/AdminNavbar";
import { AdminBottomNav } from "@/components/layout/AdminBottomNav";

export const Route = createFileRoute("/_admin")({
  component: AdminLayout,
});

function AdminLayout() {
  return (
    <div className="flex h-[100dvh] flex-col overflow-hidden bg-gradient-to-br from-background to-muted/20">
      <AdminNavbar />
      <main className="mx-auto min-h-0 w-full max-w-7xl flex-1 overflow-hidden px-5 pb-[calc(6rem+env(safe-area-inset-bottom))] pt-6 sm:px-8 sm:pt-8 lg:px-10 lg:pb-10">
        <Outlet />
      </main>
      <AdminBottomNav />
    </div>
  );
}
