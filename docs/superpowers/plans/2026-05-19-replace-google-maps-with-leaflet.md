# Replace Google Maps with Leaflet — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every Google Maps API call in `static/trip.html` with Leaflet equivalents, and remove the backend config field, API endpoint, and tests that served the Google Maps API key.

**Architecture:** Direct 1:1 API replacement — no abstraction layer. Leaflet is already loaded on the page for the forecast-area widget. The main trip map is rewritten function-by-function. Backend cleanup removes dead code.

**Tech Stack:** Leaflet 1.9.4 (already loaded), Rust/Axum (backend), Vanilla JS (frontend)

**Spec:** `docs/superpowers/specs/2026-05-19-replace-google-maps-with-leaflet.md`

---

## Files Modified

| File | Change |
|---|---|
| `src/config.rs` | Remove `google_maps_api_key` field, Default entry, env override, and two tests |
| `src/web/api.rs` | Remove `get_google_maps_key` handler, route, test, and read-only coverage entry |
| `config.example.json` | Remove `google_maps_api_key` key |
| `static/trip.html` | Replace all Google Maps JS API usage with Leaflet |

---

## Task 1: Remove google_maps_api_key from config.rs

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Run baseline tests**

  ```bash
  cargo test config::tests
  ```
  Expected: all pass.

- [ ] **Step 2: Remove the field from `WebConfig` struct and `Default` impl**

  In `src/config.rs`, in the `WebConfig` struct (around line 62), remove:
  ```rust
  /// Google Maps API key for the web interface
  pub google_maps_api_key: Option<String>,
  ```

  In the `Default` impl for `WebConfig` (around line 100), remove:
  ```rust
  google_maps_api_key: None,
  ```

- [ ] **Step 3: Remove the env-var override**

  In `apply_env_overrides` (around line 642), remove:
  ```rust
  if let Ok(key) = std::env::var("GOOGLE_MAPS_KEY") {
      self.web.google_maps_api_key = if key.is_empty() { None } else { Some(key) };
  }
  ```

- [ ] **Step 4: Fix `test_web_config_custom` — remove field from struct literal**

  Around line 1168, replace:
  ```rust
  let config = WebConfig {
      enabled: false,
      port: 9000,
      google_maps_api_key: Some("test_key".to_string()),
      auth_password: None,
      session_duration_secs: default_session_duration_secs(),
      secure_cookies: false,
      read_only: false,
  };
  assert!(!config.enabled);
  assert_eq!(config.port, 9000);
  assert_eq!(config.google_maps_api_key, Some("test_key".to_string()));
  ```
  with:
  ```rust
  let config = WebConfig {
      enabled: false,
      port: 9000,
      auth_password: None,
      session_duration_secs: default_session_duration_secs(),
      secure_cookies: false,
      read_only: false,
  };
  assert!(!config.enabled);
  assert_eq!(config.port, 9000);
  ```

- [ ] **Step 5: Fix `test_web_config_serialization` — remove field from struct literal and assertion**

  Around line 1184, replace:
  ```rust
  let config = WebConfig {
      enabled: true,
      port: 3000,
      google_maps_api_key: None,
      auth_password: None,
      session_duration_secs: default_session_duration_secs(),
      secure_cookies: true,
      read_only: false,
  };
  let json = serde_json::to_string(&config).unwrap();
  assert!(json.contains("3000"));
  assert!(json.contains("true"));

  let deserialized: WebConfig = serde_json::from_str(&json).unwrap();
  assert!(deserialized.enabled);
  assert_eq!(deserialized.port, 3000);
  assert_eq!(deserialized.google_maps_api_key, None);
  ```
  with:
  ```rust
  let config = WebConfig {
      enabled: true,
      port: 3000,
      auth_password: None,
      session_duration_secs: default_session_duration_secs(),
      secure_cookies: true,
      read_only: false,
  };
  let json = serde_json::to_string(&config).unwrap();
  assert!(json.contains("3000"));
  assert!(json.contains("true"));

  let deserialized: WebConfig = serde_json::from_str(&json).unwrap();
  assert!(deserialized.enabled);
  assert_eq!(deserialized.port, 3000);
  ```

- [ ] **Step 6: Run tests and verify they pass**

  ```bash
  cargo test config::tests
  ```
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add src/config.rs
  git commit -m "chore: remove google_maps_api_key from WebConfig"
  ```

---

## Task 2: Remove API endpoint from api.rs and config.example.json

**Files:**
- Modify: `src/web/api.rs`
- Modify: `config.example.json`

- [ ] **Step 1: Remove the `get_google_maps_key` handler function**

  In `src/web/api.rs`, remove the entire function (around line 807):
  ```rust
  pub async fn get_google_maps_key(
      State(state): State<AppState>,
  ) -> Result<Json<ApiResponse<Option<String>>>, StatusCode> {
      match state.config.web.google_maps_api_key.clone() {
          Some(key) => Ok(Json(ApiResponse::ok(Some(key)))),
          None => Ok(Json(ApiResponse::ok(None))),
      }
  }
  ```

- [ ] **Step 2: Remove the route registration**

  In the router setup (around line 1800), remove:
  ```rust
  .route("/config/google_maps_key", get(get_google_maps_key))
  ```

- [ ] **Step 3: Remove the `test_get_google_maps_key` test**

  Remove the entire test function (around line 2521):
  ```rust
  #[tokio::test]
  async fn test_get_google_maps_key() {
      let app = create_test_app();

      let response = app
          .oneshot(
              Request::builder()
                  .uri("/config/google_maps_key")
                  .body(Body::empty())
                  .unwrap(),
          )
          .await
          .unwrap();

      assert_eq!(response.status(), StatusCode::OK);

      let body = axum::body::to_bytes(response.into_body(), usize::MAX)
          .await
          .unwrap();
      let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

      assert_eq!(json["status"], "ok");
      assert_eq!(json["data"], "your_google_maps_api_key_here");
      assert!(json["error"].is_null());
  }
  ```

- [ ] **Step 4: Remove `/config/google_maps_key` from the read-only routes coverage test**

  In `test_read_only_read_routes_are_accessible` (around line 3491), remove `"/config/google_maps_key"` from the `read_routes` array:
  ```rust
  let read_routes = [
      "/trips",
      "/track?trip_id=1",
      "/metrics?metric=speed&trip_id=1",
      "/metrics/batch?metrics=speed&trip_id=1",
      "/heatmap",
      "/monthly_statistics",
      "/config/google_maps_key",   // <-- remove this line
      "/config/read_only",
  ];
  ```

- [ ] **Step 5: Remove `google_maps_api_key` from `config.example.json`**

  Open `config.example.json`. Find and remove the line:
  ```json
  "google_maps_api_key": "your_google_maps_api_key_here",
  ```
  Ensure the surrounding JSON remains valid (check for trailing comma on the preceding line).

- [ ] **Step 6: Build and test**

  ```bash
  cargo build --release 2>&1 | grep -E "^error"
  ```
  Expected: no output (no errors).

  ```bash
  cargo test -- --test-threads=1 2>&1 | tail -20
  ```
  Expected: test suite passes; no failures mentioning `google_maps`.

- [ ] **Step 7: Commit**

  ```bash
  git add src/web/api.rs config.example.json
  git commit -m "chore: remove /config/google_maps_key endpoint and dead route test"
  ```

---

## Task 3: Remove Google Maps loading infrastructure from trip.html

This task removes code that will no longer exist: the async key-fetch, the dynamic script loader, the `googleMapsLoaded` flag, the `mapsPromise` block, and the canvas fallback. No Leaflet code is added here — the page will be partially broken after this task until Task 4 is complete.

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Remove the comment in `<head>`**

  Remove line 14:
  ```html
  <!-- Google Maps API will be loaded dynamically -->
  ```

- [ ] **Step 2: Remove `googleMapsLoaded` flag and `loadGoogleMapsAPI()` function**

  Remove (around lines 355–431):
  ```js
  let googleMapsLoaded = false;

  // Load Google Maps API key dynamically with graceful fallback
  async function loadGoogleMapsAPI() {
      // ... entire function body ...
  }
  ```
  The function ends just before `function showMapFallback(message)` at line 435. Remove everything from `let googleMapsLoaded = false;` up to (but not including) `function showMapFallback`.

- [ ] **Step 3: Remove the `mapsPromise` lines from `loadTripAndLegs`**

  In `loadTripAndLegs` (around line 647), remove:
  ```js
  // Kick off Google Maps load in the background — do NOT await it here
  // so the data fetches can proceed in parallel
  const mapsPromise = loadGoogleMapsAPI().catch(() => false);
  ```

  Further down in the same function (around line 706), remove:
  ```js
  // Wait for Google Maps to finish (it may still be loading)
  const mapsLoaded = await mapsPromise;

  // If Maps finished loading after initializeMap() already ran (and showed
  // the fallback), render the map now with the track data we already have.
  if (mapsLoaded && !currentMap && currentTrackData) {
      initializeMap(currentTrackData, currentIsFullTrip, currentNavStartTs, currentNavEndTs);
  }
  ```

- [ ] **Step 4: Remove `drawCanvasTrack()` function**

  Remove the entire `drawCanvasTrack` function (lines 2377–2509):
  ```js
  function drawCanvasTrack(trackData, isFullTrip) {
      // ... entire function body (~130 lines) ...
  }
  ```
  It starts with `function drawCanvasTrack(trackData, isFullTrip) {` and ends just before `function initializeMap(trackData, isFullTrip, navStartTs, navEndTs) {`.

- [ ] **Step 5: Commit**

  ```bash
  git add static/trip.html
  git commit -m "chore: remove Google Maps loading infrastructure from trip.html"
  ```

---

## Task 4: Rewrite `initializeMap()` and `createColoredTrack()` with Leaflet

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Rewrite `initializeMap()`**

  Replace the entire `initializeMap` function body with the Leaflet version. The function signature stays the same: `function initializeMap(trackData, isFullTrip, navStartTs, navEndTs)`.

  Replace the opening of the function — from the nav-legend reset block through to the end of the function — with:

  ```js
  function initializeMap(trackData, isFullTrip, navStartTs, navEndTs) {
      const navStartEl = document.getElementById('navStartLegendItem');
      const navEndEl   = document.getElementById('navEndLegendItem');
      if (navStartEl) navStartEl.style.display = 'none';
      if (navEndEl)   navEndEl.style.display   = 'none';

      // Destroy existing map before re-initialising (Leaflet throws on double-init)
      if (currentMap) {
          currentMap.remove();
          currentMap = null;
      }

      const validTrackPoints = trackData.filter(point =>
          point.latitude !== null && point.latitude !== undefined &&
          point.longitude !== null && point.longitude !== undefined
      );

      if (validTrackPoints.length === 0) {
          showMapFallback('No GPS track data available for this trip');
          return;
      }

      let minLat = Infinity, maxLat = -Infinity, minLng = Infinity, maxLng = -Infinity;
      validTrackPoints.forEach(point => {
          minLat = Math.min(minLat, point.latitude);
          maxLat = Math.max(maxLat, point.latitude);
          minLng = Math.min(minLng, point.longitude);
          maxLng = Math.max(maxLng, point.longitude);
      });

      const latPadding = (maxLat - minLat) * 0.1 || 0.01;
      const lngPadding = (maxLng - minLng) * 0.1 || 0.01;

      const map = L.map(document.getElementById('map')).setView(
          [(minLat + maxLat) / 2, (minLng + maxLng) / 2], 12
      );
      L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
          attribution: '© OpenStreetMap contributors',
          maxZoom: 19
      }).addTo(map);

      currentMap = map;

      map.on('click', () => resetSegmentMarkers());

      map.fitBounds([
          [minLat - latPadding, minLng - lngPadding],
          [maxLat + latPadding, maxLng + lngPadding]
      ]);

      const hoverPopup = L.popup({ closeButton: false, autoPan: false });
      createColoredTrack(map, validTrackPoints, hoverPopup);

      if (!isFullTrip) {
          addHourlyMarkers(map, validTrackPoints);
      }

      if (validTrackPoints.length > 0) {
          const flagIcon = (color) => L.divIcon({
              html: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="-10 -50 20 50" width="14" height="35">' +
                    '<path d="M 0,0 C -2,-20 -10,-22 -10,-30 L -10,-50 L 10,-50 L 10,-30 C 10,-22 2,-20 0,0 z" ' +
                    'fill="' + color + '" stroke="#FFFFFF" stroke-width="2"/></svg>',
              iconSize: [14, 35],
              iconAnchor: [7, 35],
              className: ''
          });

          const startPt = validTrackPoints[0];
          const endPt   = validTrackPoints[validTrackPoints.length - 1];

          L.marker([startPt.latitude, startPt.longitude], { icon: flagIcon('#00FF00'), title: 'Trip Start', zIndexOffset: 1000 })
              .addTo(map)
              .bindPopup('<div style="font-family: Arial, sans-serif;"><strong>Trip Start</strong><br>' + formatDate(startPt.timestamp) + '</div>');

          L.marker([endPt.latitude, endPt.longitude], { icon: flagIcon('#FF0000'), title: 'Trip End', zIndexOffset: 1000 })
              .addTo(map)
              .bindPopup('<div style="font-family: Arial, sans-serif;"><strong>Trip End</strong><br>' + formatDate(endPt.timestamp) + '</div>');
      }

      if (!isFullTrip && (navStartTs || navEndTs)) {
          addNavWindowMarkers(map, validTrackPoints, navStartTs, navEndTs);
      }
  }
  ```

- [ ] **Step 2: Rewrite `createColoredTrack()`**

  Replace the entire function:

  ```js
  function createColoredTrack(map, trackPoints, hoverPopup) {
      if (trackPoints.length < 2) return;

      for (let i = 0; i < trackPoints.length - 1; i++) {
          const point1 = trackPoints[i];
          const point2 = trackPoints[i + 1];

          let color;
          if (point2.engine_on === 1) {
              color = '#888888';
          } else if (point2.engine_on === 2) {
              color = '#FFD700';
          } else {
              const speed = point2.avg_speed_kn || point1.avg_speed_kn || 0;
              color = getSpeedColor(speed);
          }

          const segmentLine = L.polyline(
              [[point1.latitude, point1.longitude], [point2.latitude, point2.longitude]],
              { color, weight: 4, opacity: 1.0 }
          ).addTo(map);

          if (hoverPopup) {
              const tooltipPoint = point2;
              segmentLine.on('mouseover', (e) => {
                  const time    = tooltipPoint.timestamp ? formatTime(tooltipPoint.timestamp) : 'N/A';
                  const speed   = tooltipPoint.avg_speed_kn != null ? tooltipPoint.avg_speed_kn.toFixed(1) + ' kn' : 'N/A';
                  const heading = tooltipPoint.average_heading_deg != null ? tooltipPoint.average_heading_deg.toFixed(0) + '°' : 'N/A';
                  const wind    = tooltipPoint.average_wind_speed_kn != null ? tooltipPoint.average_wind_speed_kn.toFixed(1) + ' kn' : 'N/A';
                  let absWindDir;
                  if (tooltipPoint.absolute_wind_direction_deg != null && tooltipPoint.average_heading_deg != null) {
                      let d = (tooltipPoint.absolute_wind_direction_deg + tooltipPoint.average_heading_deg) % 360;
                      if (d < 0) d += 360;
                      absWindDir = d.toFixed(0) + '°';
                  } else {
                      absWindDir = 'N/A';
                  }
                  hoverPopup
                      .setLatLng(e.latlng)
                      .setContent(
                          '<div class="map-track-tooltip">' +
                          '<div class="tooltip-time">' + time + '</div>' +
                          '<div><span class="tooltip-label">Speed </span><span class="tooltip-value">' + speed + ' ' + heading + '</span></div>' +
                          '<div><span class="tooltip-label">Wind </span><span class="tooltip-value">' + wind + ' ' + absWindDir + '</span></div>' +
                          '</div>'
                      )
                      .openOn(map);
              });
              segmentLine.on('mouseout', () => {
                  map.closePopup(hoverPopup);
              });
          }
      }
  }
  ```

- [ ] **Step 3: Verify no remaining `google.maps` references in the rewritten functions**

  ```bash
  grep -n "google\.maps" static/trip.html
  ```
  Expected output: only lines in `addHourlyMarkers`, `addNavWindowMarkers`, and `displaySegmentMarkers` (not yet converted).

- [ ] **Step 4: Commit**

  ```bash
  git add static/trip.html
  git commit -m "feat: rewrite initializeMap and createColoredTrack using Leaflet"
  ```

---

## Task 5: Rewrite `addHourlyMarkers()` with Leaflet

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Replace the marker creation inside `addHourlyMarkers`**

  The function loop builds an `svg` string and then creates a marker. Replace only the marker creation at the bottom of the loop. Change from:

  ```js
  const marker = new google.maps.Marker({
      position: { lat: closest.latitude, lng: closest.longitude },
      map: map,
      title: hourLabel + ' UTC',
      icon: {
          url: 'data:image/svg+xml;charset=UTF-8,' + encodeURIComponent(svg),
          size: new google.maps.Size(50, 32),
          origin: new google.maps.Point(0, 0),
          anchor: new google.maps.Point(25, 28),
          scaledSize: new google.maps.Size(50, 32)
      },
      zIndex: 500
  });
  ```

  to:

  ```js
  L.marker([closest.latitude, closest.longitude], {
      icon: L.icon({
          iconUrl: 'data:image/svg+xml;charset=UTF-8,' + encodeURIComponent(svg),
          iconSize: [50, 32],
          iconAnchor: [25, 28]
      }),
      title: hourLabel + ' UTC',
      zIndexOffset: 500
  }).addTo(map);
  ```

- [ ] **Step 2: Verify**

  ```bash
  grep -n "google\.maps" static/trip.html
  ```
  Expected: only lines in `addNavWindowMarkers` and `displaySegmentMarkers` remain.

- [ ] **Step 3: Commit**

  ```bash
  git add static/trip.html
  git commit -m "feat: rewrite addHourlyMarkers using Leaflet"
  ```

---

## Task 6: Rewrite `addNavWindowMarkers()` with Leaflet

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Replace the entire `addNavWindowMarkers` function**

  ```js
  function addNavWindowMarkers(map, trackPoints, navStartTs, navEndTs) {
      function findClosestPoint(ts) {
          const targetMs = new Date(ts).getTime();
          let best = null, bestDiff = Infinity;
          for (const pt of trackPoints) {
              const diff = Math.abs(new Date(pt.timestamp).getTime() - targetMs);
              if (diff < bestDiff) { bestDiff = diff; best = pt; }
          }
          return best;
      }

      if (navStartTs) {
          const pt = findClosestPoint(navStartTs);
          if (pt) {
              L.circleMarker([pt.latitude, pt.longitude], {
                  radius: 8,
                  fillColor: '#0099FF',
                  fillOpacity: 1,
                  color: '#FFFFFF',
                  weight: 2
              })
              .addTo(map)
              .bindPopup(
                  '<div style="font-family: Arial, sans-serif;">' +
                  '<strong>Navigation Start</strong><br>' +
                  formatTime(navStartTs) +
                  '</div>'
              );
              const el = document.getElementById('navStartLegendItem');
              if (el) el.style.display = '';
          }
      }

      if (navEndTs) {
          const pt = findClosestPoint(navEndTs);
          if (pt) {
              L.circleMarker([pt.latitude, pt.longitude], {
                  radius: 8,
                  fillColor: '#FF6600',
                  fillOpacity: 1,
                  color: '#FFFFFF',
                  weight: 2
              })
              .addTo(map)
              .bindPopup(
                  '<div style="font-family: Arial, sans-serif;">' +
                  '<strong>Navigation End</strong><br>' +
                  formatTime(navEndTs) +
                  '</div>'
              );
              const el = document.getElementById('navEndLegendItem');
              if (el) el.style.display = '';
          }
      }
  }
  ```

- [ ] **Step 2: Verify**

  ```bash
  grep -n "google\.maps" static/trip.html
  ```
  Expected: only lines in `displaySegmentMarkers` remain.

- [ ] **Step 3: Commit**

  ```bash
  git add static/trip.html
  git commit -m "feat: rewrite addNavWindowMarkers using Leaflet"
  ```

---

## Task 7: Rewrite `displaySegmentMarkers()` and `resetSegmentMarkers()` with Leaflet

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Rewrite the polyline creation in `displaySegmentMarkers`**

  In `displaySegmentMarkers`, replace the `if (segmentPoints.length > 0)` block. Change from:

  ```js
  if (segmentPoints.length > 0) {
      const segmentPath = segmentPoints
          .filter(point => point.latitude !== null && point.longitude !== null)
          .map(point => ({ lat: point.latitude, lng: point.longitude }));

      const segmentLine = new google.maps.Polyline({
          path: segmentPath,
          geodesic: true,
          strokeColor: '#FF8800',
          strokeOpacity: 1.0,
          strokeWeight: 6,
          map: currentMap,
          zIndex: 2000
      });

      segmentMarkers.push(segmentLine);

      let minLat = Infinity, maxLat = -Infinity, minLng = Infinity, maxLng = -Infinity;
      segmentPath.forEach(point => {
          minLat = Math.min(minLat, point.lat);
          maxLat = Math.max(maxLat, point.lat);
          minLng = Math.min(minLng, point.lng);
          maxLng = Math.max(maxLng, point.lng);
      });

      const latPadding = (maxLat - minLat) * 0.3 || 0.01;
      const lngPadding = (maxLng - minLng) * 0.3 || 0.01;

      const segmentBounds = {
          north: maxLat + latPadding,
          south: minLat - latPadding,
          east: maxLng + lngPadding,
          west: minLng - lngPadding
      };

      currentMap.fitBounds(segmentBounds);
  }
  ```

  to:

  ```js
  if (segmentPoints.length > 0) {
      const latlngs = segmentPoints
          .filter(point => point.latitude !== null && point.longitude !== null)
          .map(point => [point.latitude, point.longitude]);

      const segmentLine = L.polyline(latlngs, {
          color: '#FF8800',
          weight: 6,
          opacity: 1.0
      }).addTo(currentMap);

      segmentMarkers.push(segmentLine);

      let minLat = Infinity, maxLat = -Infinity, minLng = Infinity, maxLng = -Infinity;
      latlngs.forEach(([lat, lng]) => {
          minLat = Math.min(minLat, lat);
          maxLat = Math.max(maxLat, lat);
          minLng = Math.min(minLng, lng);
          maxLng = Math.max(maxLng, lng);
      });

      const latPadding = (maxLat - minLat) * 0.3 || 0.01;
      const lngPadding = (maxLng - minLng) * 0.3 || 0.01;

      currentMap.fitBounds([
          [minLat - latPadding, minLng - lngPadding],
          [maxLat + latPadding, maxLng + lngPadding]
      ]);
  }
  ```

- [ ] **Step 2: Rewrite `resetSegmentMarkers` — replace `setMap(null)` with `remove()`**

  Change from:
  ```js
  for (let marker of segmentMarkers) {
      marker.setMap(null);
  }
  ```
  to:
  ```js
  for (let marker of segmentMarkers) {
      marker.remove();
  }
  ```

- [ ] **Step 3: Verify no remaining Google Maps references**

  ```bash
  grep -n "google\.maps\|googleMapsLoaded\|loadGoogleMapsAPI\|mapsPromise\|drawCanvasTrack" static/trip.html
  ```
  Expected: no output.

- [ ] **Step 4: Commit**

  ```bash
  git add static/trip.html
  git commit -m "feat: rewrite displaySegmentMarkers and resetSegmentMarkers using Leaflet"
  ```

---

## Task 8: Final verification

- [ ] **Step 1: Full build**

  ```bash
  cargo build --release 2>&1 | grep -E "^error"
  ```
  Expected: no output.

- [ ] **Step 2: Full test suite**

  ```bash
  cargo test -- --test-threads=1 2>&1 | tail -30
  ```
  Expected: all tests pass; no failures.

- [ ] **Step 3: Manual browser verification**

  Open `http://localhost:8080/trip.html?id=<any-trip-id>` and verify:
  - Map renders with OSM tiles (no Google Maps watermark)
  - Track polyline renders with speed/engine colouring
  - Hovering a track segment shows the tooltip (time, speed, heading, wind)
  - Start (green flag) and End (red flag) markers are visible and show popup on click
  - Switch to a single-leg view: hourly time markers appear
  - If a leg has a nav window: blue/orange circle markers appear and show popup on click
  - Click a fastest-segment card in the analytics panel: map zooms to the highlighted orange segment
  - No browser console errors mentioning `google` or `GoogleMaps`
