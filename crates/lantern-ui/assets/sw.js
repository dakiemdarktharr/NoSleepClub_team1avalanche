/* Cache static assets only. Never cache navigation, private state, or API data. */
const CACHE = 'lantern-static-v5';
const STATIC = ['/assets/app.css','/assets/app.js','/assets/icon.svg','/manifest.webmanifest'];
self.addEventListener('install', event => { event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(STATIC)).then(() => self.skipWaiting())); });
self.addEventListener('activate', event => { event.waitUntil(caches.keys().then(keys => Promise.all(keys.filter(key => key.startsWith('lantern-static-') && key !== CACHE).map(key => caches.delete(key)))).then(() => self.clients.claim())); });
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET' || url.origin !== self.location.origin || !STATIC.includes(url.pathname)) return;
  event.respondWith(caches.match(event.request).then(hit => hit || fetch(event.request)));
});
