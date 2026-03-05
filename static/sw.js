// Service Worker for NMEA Router
// Provides offline support and graceful degradation for missing external resources

const CACHE_NAME = 'nmea-router-v1';

// Install event - cache essential assets
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => {
      return cache.addAll([
        '/',
        '/index.html',
        '/trip.html',
        '/shared.css',
        '/shared-theme.js',
        '/nmeasail.png'
      ]).catch((err) => {
        console.log('Some assets could not be cached:', err);
      });
    })
  );
  self.skipWaiting();
});

// Activate event - clean up old caches
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((cacheNames) => {
      return Promise.all(
        cacheNames.map((cacheName) => {
          if (cacheName !== CACHE_NAME) {
            return caches.delete(cacheName);
          }
        })
      );
    })
  );
  self.clients.claim();
});

// Fetch event - implement caching strategies
self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle GET requests
  if (request.method !== 'GET') {
    return;
  }

  // Local API calls: Network first, fallback to cache
  if (url.origin === location.origin && url.pathname.startsWith('/api/')) {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.status === 200) {
            const cloned = response.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
          }
          return response;
        })
        .catch(() => {
          // Server is down, try cache
          return caches.match(request).then((cached) => {
            return cached || new Response(
              JSON.stringify({ error: 'Server unavailable', status: 'offline' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, graceful degradation
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch with timeout
          return Promise.race([
            fetch(request),
            new Promise((_, reject) =>
              setTimeout(() => reject(new Error('timeout')), 5000)
            )
          ])
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder instead of failing
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { status: 200, headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { status: 200, headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('', { status: 204 });
            });
        })
    );
    return;
  }

  // Static assets (local): Cache with network update
  event.respondWith(
    caches.match(request)
      .then((cached) => {
        const fetchPromise = fetch(request)
          .then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          })
          .catch(() => cached);
        
        return cached || fetchPromise;
      })
  );
});
