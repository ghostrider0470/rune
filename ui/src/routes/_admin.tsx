import { createFileRoute, Outlet } from "@tanstack/react-router";
import { AdminNavbar } from "@/components/layout/AdminNavbar";
import { AdminBottomNav } from "@/components/layout/AdminBottomNav";

export const Route = createFileRoute("/_admin")({
  component: AdminLayout,
});

function AdminLayout() {
  return (
    <div className="admin-shell flex min-h-[100dvh] flex-col bg-gradient-to-br from-background to-muted/20">
      <AdminNavbar />
      <main className="mx-auto w-full max-w-7xl flex-1 px-4 pb-[calc(6.5rem+env(safe-area-inset-bottom))] pt-4 sm:px-6 sm:pt-6 lg:px-10 lg:pb-10 lg:pt-8">
        <Outlet />
      </main>
      <AdminBottomNav />
    </div>
  );
}
