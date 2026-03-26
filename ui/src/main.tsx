import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { QueryProvider, queryClient } from "@/integrations/tanstack-query/root-provider";
import { routeTree } from "./routeTree.gen";
import { registerServiceWorker } from "@/lib/register-sw";
import "./styles.css";
import { Toaster } from "@/components/ui/sonner";

registerServiceWorker();

const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <QueryProvider>
      <RouterProvider router={router} />
      <Toaster richColors closeButton />
    </QueryProvider>
  </StrictMode>
);
