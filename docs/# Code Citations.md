# Code Citations

## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```


## License: unknown
https://github.com/liangchaofei/node_crawler/blob/1ca9afb783d2316e7444e02ff77352aee935a3eb/02/static/index.html

```
# Making the UI Resilient to Missing Internet (But With Server Connection)

You're right—the core distinction is important. The server is accessible locally, but external resources (CDNs, APIs, map libraries) may fail. Here's how to make the UI work offline:

## 1. **Remove External Dependencies**

First, audit what's currently loaded from the internet:

````html
<!-- REMOVE these if present in index.html -->
<!-- ❌ Don't load from CDN -->
<link rel="stylesheet" href="https://cdn.example.com/library.css">
<script src="https://maps.googleapis.com/maps/api/js?key=..."></script>

<!-- ✅ DO load from local server -->
<link rel="stylesheet" href="/libs/library.css">
<script src="/libs/maps.js"></script>
````

## 2. **Cache External Resources Locally**

For any third-party libraries you need (e.g., chart libraries, date pickers):

- Download them and serve from `/static/libs/`
- Reference them locally in HTML

````html
<!-- shared.css - update paths -->
<link rel="stylesheet" href="/libs/chart.min.css">
<script src="/libs/chart.min.js"></script>
````

## 3. **Graceful Fallback for Missing Features**

If you do need external resources, wrap them in try-catch:

````javascript
// Example: If you were using an external map library
async function loadMapLibrary() {
    try {
        // Try to load from CDN first
        const script = document.createElement('script');
        script.src = 'https://maps.googleapis.com/maps/api/js?key=YOUR_KEY';
        script.async = true;
        script.timeout = 3000;  // 3 second timeout
        
        await new Promise((resolve, reject) => {
            script.onload = resolve;
            script.onerror = reject;
            document.head.appendChild(script);
        });
    } catch (error) {
        console.warn('External map library unavailable, using fallback', error);
        // Use fallback or disable map features
        document.getElementById('mapContainer').innerHTML = 
            '<div class="error">Map unavailable - using list view instead</div>';
    }
}
````

## 4. **Update Service Worker for Local-Only Mode**

Modify the Service Worker to distinguish between server-accessible and internet-accessible resources:

````javascript
// filepath: /home/aboni/dev/rust_nmea_router/static/sw.js
const CACHE_NAME = 'nmea-router-v1';
const API_ENDPOINTS = ['/api/'];  // Local server endpoints only
const OFFLINE_ROUTES = ['/', '/index.html', '/trip.html'];

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

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
              JSON.stringify({ error: 'Server unavailable' }),
              { status: 503, headers: { 'Content-Type': 'application/json' } }
            );
          });
        })
    );
    return;
  }

  // External resources (CDN, maps, etc.): Cache first, don't fail the page
  if (url.origin !== location.origin) {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          if (cached) return cached;
          
          // Try to fetch, but don't block page if it fails
          return fetch(request)
            .then((response) => {
              if (response.status === 200) {
                const cloned = response.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
              }
              return response;
            })
            .catch(() => {
              // External resource unavailable - return empty/placeholder
              if (request.destination === 'style') {
                return new Response('/* External CSS unavailable */', 
                  { headers: { 'Content-Type': 'text/css' } });
              }
              if (request.destination === 'script') {
                return new Response('/* External script unavailable */', 
                  { headers: { 'Content-Type': 'text/javascript' } });
              }
              return new Response('Resource unavailable', { status: 503 });
            });
        })
    );
    return;
  }

  // Static assets: Cache with network update
  if (request.method === 'GET') {
    event.respondWith(
      caches.match(request)
        .then((cached) => {
          const fetchPromise = fetch(request).then((response) => {
            if (response.status === 200) {
              const cloned = response.clone();
              caches.open(CACHE_NAME).then((cache) => cache.put(request, cloned));
            }
            return response;
          }).catch(() => cached);
          
          return cached || fetchPromise;
        })
    );
  }
});
````

## 5. **Update index.html for No External Dependencies**

````html
<!-- filepath: /home/aboni/dev/rust_nmea_router/static/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NMEA Router Dashboard</title>
    <link rel="icon" type="image/png
```

