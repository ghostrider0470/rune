/// <reference lib="webworker" />

const CACHE_NAME = "rune-shell-v1";

/**
 * Static shell assets cached on install for offline loading.
 * The root document and key static assets are precached;
 * additional build-hashed assets are cached on first fetch.
 */
const PRECACHE_URLS = [
  "/",
  "/manifest.json",
  "/favicon.svg",
  "/icons.svg",
  "/pwa-icon-192.png",
];

/* ---------- install: precache shell assets ---------- */

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll(PRECACHE_URLS))
      .then(() => self.skipWaiting()),
  );
});

/* ---------- activate: purge old caches ---------- */

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key !== CACHE_NAME)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

/* ---------- fetch: cache-first for shell, network-first for API ---------- */

self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Skip non-GET requests
  if (request.method !== "GET") return;

  // Skip API/WebSocket routes — always go to network
  if (
    url.pathname.startsWith("/api") ||
    url.pathname.startsWith("/ws") ||
    url.pathname.startsWith("/health") ||
    url.pathname.startsWith("/status")
  ) {
    return;
  }

  // For navigation requests (HTML pages), try network first, fall back to
  // cached shell so the SPA can boot offline.
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request)
        .then((response) => {
          const clone = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
          return response;
        })
        .catch(() => caches.match("/") || caches.match(request)),
    );
    return;
  }

  // Static assets (JS/CSS/images): cache-first, falling back to network.
  // Vite-hashed assets are immutable so cache-first is safe.
  event.respondWith(
    caches.match(request).then(
      (cached) =>
        cached ||
        fetch(request).then((response) => {
          // Only cache successful same-origin responses
          if (response.ok && url.origin === self.location.origin) {
            const clone = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
          }
          return response;
        }),
    ),
  );
});
