// Service worker: keeps the app working offline once it has loaded.
//
// Network first, so a new build (or a file being edited during
// development) is picked up on the next load; the cache answers only when
// the network cannot. `cargo xtask web` stamps BUILD with a hash of the
// files, so each build starts a fresh cache and old ones are removed.

const BUILD = "dev";
const CACHE = `cad-${BUILD}`;
const SHELL = [
  "./",
  "index.html",
  "style.css",
  "app.js",
  "core.js",
  "icons.js",
  "core.wasm",
  "manifest.webmanifest",
  "icons/icon.svg",
  "icons/icon-192.png",
  "icons/icon-512.png",
];

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k.startsWith("cad-") && k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (e) => {
  const req = e.request;
  if (req.method !== "GET" || new URL(req.url).origin !== location.origin) return;
  e.respondWith(
    fetch(req)
      .then((res) => {
        if (res.ok) {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(req, copy));
        }
        return res;
      })
      .catch(() => caches.match(req, { ignoreSearch: true }).then((r) => r || caches.match("index.html")))
  );
});
