import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import path from "path";

export default defineConfig({
  plugins: [
    TanStackRouterVite({ autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/health": "http://127.0.0.1:18790",
      "/status": "http://127.0.0.1:18790",
      "/api": "http://127.0.0.1:18790",
      "/sessions": "http://127.0.0.1:18790",
      "/cron": "http://127.0.0.1:18790",
      "/approvals": "http://127.0.0.1:18790",
      "/heartbeat": "http://127.0.0.1:18790",
      "/reminders": "http://127.0.0.1:18790",
      "/gateway": "http://127.0.0.1:18790",
      "/assets": "http://127.0.0.1:18790",
      "/webhook": "http://127.0.0.1:18790",
      "/ws": {
        target: "http://127.0.0.1:18790",
        ws: true,
      },
    },
  },
});
