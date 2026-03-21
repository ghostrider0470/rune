/**
 * Register the service worker for offline PWA shell support.
 * Called once at app boot from main.tsx.
 */
export function registerServiceWorker(): void {
  if (!("serviceWorker" in navigator)) return;

  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch((err) => {
      console.warn("[rune] service-worker registration failed:", err);
    });
  });
}
