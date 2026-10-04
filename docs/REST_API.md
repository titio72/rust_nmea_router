# NMEA Router REST API Documentation

## Overview

The NMEA Router provides a RESTful API for accessing vessel tracking data, trip information, environmental metrics, weather forecasts and runtime controls. The API is built with Axum and returns JSON.

**Base URL**: `http://localhost:8080/api`

**Default Port**: 8080 (configurable in `config.json`)

### Read-only mode

When `web.read_only` is `true` only the read endpoints are registered: everything under **Trips and Statistics**, **Navigation and Environment** and **Health and Configuration**, plus `GET /api/export_trip`, `GET /api/sync/status`, `POST /api/sync/manifest` and `POST /api/sync/trip`. The sections **Trip Editing** (except Export Trip), **Runtime Toggles**, **Backups and System** and **Forecast**, plus `POST /api/sync/push`, are not registered and requests to them return `404`. Endpoints that are only available when `read_only` is `false` are tagged **[write]**.

### Authentication

If `web.auth_password` is set, all `/api/*` requests need a valid session cookie, except the public paths `/api/auth/*`, `/api/health`, `/api/sync/manifest` and `/api/sync/trip` (the last two use their own bearer token, see [Sync](#sync)). Unauthenticated requests receive `401` with `{"status":"error","error":"Unauthorized"}`. See [Authentication endpoints](#authentication-endpoints).

### Date/time parameters

Every `start`, `end`, `*_timestamp` and `timestamp` parameter accepts either RFC 3339 (`2026-02-01T00:00:00Z`) or a UTC SQL-style datetime (`2026-02-01 00:00:00`). A timestamp without a timezone suffix, such as `2026-02-01T00:00:00`, is rejected with HTTP `400`. In query strings, encode the space as `%20` or `+`.

### `max_points`

`/api/track`, `/api/metrics` and `/api/metrics/batch` accept an optional `max_points` (integer) to downsample the result. Values above 10,000 are clamped to 10,000.

## Response Format

Most responses use a common envelope:

```json
{
  "status": "ok" | "error",
  "data": <response_data> | null,
  "error": "<error_message>" | null
}
```

Application-level failures (not found, database error, validation) normally return HTTP `200` with `"status": "error"`. HTTP status codes are used for malformed input (`400`), authentication (`401`), missing routes (`404`) and a few specific cases noted below.

Exceptions that do **not** use the envelope are marked in their section: `GET /api/config/read_only`, `GET /api/config/capabilities`, `GET /api/health`, `GET /api/auth/status`, `GET /api/backup/download`.

### Success Response
```json
{ "status": "ok", "data": { ... }, "error": null }
```

### Error Response
```json
{ "status": "error", "data": null, "error": "Error description" }
```

---

## Endpoints

## Trips and Statistics

### Get Trips

**Endpoint**: `GET /api/trips`

**Query Parameters**:
- `year` (optional, integer): Only trips in that year
- `last_months` (optional, integer): Only trips from the last N months

**Example**:
```bash
curl "http://localhost:8080/api/trips?last_months=3"
```

**Response**: array of trip summaries (see [Get Trip](#get-trip)).

---

### Get Trip

**Endpoint**: `GET /api/trip`

**Query Parameters**:
- `id` (required, integer): Trip ID

**Example**:
```bash
curl "http://localhost:8080/api/trip?id=132"
```

**Response**:
```json
{
  "status": "ok",
  "data": {
    "id": 132,
    "uuid": "6f1c0a52-...",
    "description": "Weekend sail",
    "start_date": "2026-02-01T08:00:00.000Z",
    "end_date": "2026-02-02T17:30:00.000Z",
    "total_distance_nm": 52.4,
    "total_time_ms": 120600000,
    "sailing_time_ms": 90000000,
    "motoring_time_ms": 20000000,
    "moored_time_ms": 10600000,
    "sailing_distance_nm": 40.1,
    "motoring_distance_nm": 12.3,
    "upwind_distance_nm": 10.0,
    "reaching_distance_nm": 22.0,
    "running_distance_nm": 8.1,
    "upwind_time_ms": 20000000,
    "reaching_time_ms": 45000000,
    "running_time_ms": 25000000
  },
  "error": null
}
```

An unknown ID returns `"status": "error"` with `"Trip <id> not found"`.

---

### Get Trip by UUID

**Endpoint**: `GET /api/trip_by_uuid`

**Query Parameters**:
- `uuid` (required, string): Trip UUID

Response is the same trip summary as [Get Trip](#get-trip).

---

### Get Track Data

GPS track points, filtered by trip or date range.

**Endpoint**: `GET /api/track`

**Query Parameters**:
- `trip_id` (optional, integer): Filter by trip
- `start` / `end` (optional, string): Date range
- `max_points` (optional, integer): Downsample limit (max 10,000)

**Example**:
```bash
curl "http://localhost:8080/api/track?trip_id=132&max_points=2000"
curl "http://localhost:8080/api/track?start=2026-02-01T00:00:00Z&end=2026-02-03T23:59:59Z"
```

**Response** (`data` is an array):
```json
{
  "timestamp": "2026-02-01T08:00:30.000Z",
  "latitude": 45.5231,
  "longitude": 12.3456,
  "avg_speed_kn": 5.2,
  "max_speed_kn": 6.0,
  "moored": false,
  "engine_on": 0,
  "total_distance_nm": 0.04,
  "total_time_ms": 30000,
  "average_wind_speed_kn": 12.1,
  "average_wind_angle_deg": 45.0,
  "cog_deg": 182.0,
  "average_heading_deg": 180.5,
  "polar_speed_kn": 5.8,
  "polar_ratio": 89.7
}
```

`engine_on` is `0` off, `1` on, `2` unknown. `polar_speed_kn` and `polar_ratio` (percentage of polar speed) are only present when a polar file is configured and wind data is available. Position, speed and wind fields can be `null`.

---

### Get Speed Distribution

Distance sailed and motored per speed bucket (0.5 kn buckets).

**Endpoint**: `GET /api/speed_distribution`

**Query Parameters**: `id` (optional, trip ID), `start`, `end` (optional)

**Response**:
```json
{ "status": "ok", "data": { "labels": ["0-0.5", "0.5-1.0"], "sailing": [0.0, 1.2], "motoring": [0.3, 0.0] }, "error": null }
```

---

### Get Wind Statistics

**Endpoint**: `GET /api/wind_statistics`

**Query Parameters**: `id` (optional, trip ID), `start`, `end` (optional)

**Response** (arrays are parallel, one entry per wind direction bucket):
```json
{ "status": "ok", "data": { "directions": [0.0, 22.5], "wind_distances": [3.1, 5.0], "max_wind_speeds": [14.0, 18.5] }, "error": null }
```

---

### Get TWA Distribution

Distance sailed per signed true-wind-angle bucket.

**Endpoint**: `GET /api/twa_distribution`

**Query Parameters**: `id` (optional, trip ID), `start`, `end` (optional)

**Response**:
```json
{ "status": "ok", "data": { "angles": [-180.0, -175.0], "distance": [0.0, 0.4] }, "error": null }
```
`angles` are bucket lower bounds in degrees (−180 to 175); negative is port, positive is starboard. `distance` is in nautical miles.

---

### Get Compass Deviation

Mean difference between heading and COG per 10° heading bucket, over underway samples.

**Endpoint**: `GET /api/compass_deviation`

**Query Parameters**:
- `start` (required, string)
- `end` (required, string)
- `min_speed_kn` (optional, number, default `5.0`): Ignore samples slower than this

**Response**:
```json
{ "status": "ok", "data": [ { "heading": 0.0, "count": 120, "mean_diff": -2.1 } ], "error": null }
```
`mean_diff` is `null` for a bucket with no samples.

---

### Get Trip Legs

Legs of a trip (segments between mooring events), with sailing/motoring breakdown, navigation window and fastest-segment records.

**Endpoint**: `GET /api/trip_legs`

**Query Parameters**:
- `id` (required, integer): Trip ID

**Response**:
```json
{
  "status": "ok",
  "data": {
    "legs": [
      {
        "leg_number": 1,
        "start_timestamp": "2026-02-01T08:00:00.000Z",
        "end_timestamp": "2026-02-01T16:00:00.000Z",
        "total_distance_nm": 30.2,
        "sailing_distance_nm": 25.0,
        "motoring_distance_nm": 5.2,
        "sailing_time_ms": 20000000,
        "motoring_time_ms": 5000000,
        "sailing_time_formatted": "5h 33m",
        "motoring_time_formatted": "1h 23m",
        "upwind_distance_nm": 8.0,
        "reaching_distance_nm": 12.0,
        "running_distance_nm": 5.0,
        "upwind_time_ms": 7000000,
        "reaching_time_ms": 8000000,
        "running_time_ms": 5000000,
        "start_lat": 45.1, "start_lon": 12.1,
        "end_lat": 45.4, "end_lon": 12.6,
        "nav_start_timestamp": "2026-02-01T08:20:00.000Z",
        "nav_end_timestamp": "2026-02-01T15:40:00.000Z",
        "nav_distance_nm": 29.0,
        "nav_time_ms": 26400000,
        "nav_detection_method": "engine_transition",
        "max_speed_kn": 8.4,
        "max_speed_timestamp": "2026-02-01T12:10:00.000Z",
        "fastest_1nm":  { "distance_nm": 1.0, "average_speed_kn": 8.1, "duration_ms": 444000, "start_timestamp": "...", "end_timestamp": "..." },
        "fastest_5nm": null,
        "fastest_10nm": null,
        "fastest_25nm": null
      }
    ]
  },
  "error": null
}
```
`nav_detection_method` is `"engine_transition"`, `"speed_fallback"` or `null`. The `fastest_*` records are `null` when the leg is shorter than that distance.

---

### Get Monthly Statistics

**Endpoint**: `GET /api/monthly_statistics`

**Response**:
```json
{
  "status": "ok",
  "data": {
    "months": [
      {
        "year": 2026, "month": 2, "date": "2026-02",
        "sailing_distance_nm": 120.5, "motoring_distance_nm": 30.0,
        "upwind_distance_nm": 40.0, "reaching_distance_nm": 60.0, "running_distance_nm": 20.5,
        "upwind_time_ms": 36000000, "reaching_time_ms": 50000000, "running_time_ms": 18000000
      }
    ]
  },
  "error": null
}
```

---

### Get Heatmap

Daily distance for the 365 days ending at the given date.

**Endpoint**: `GET /api/heatmap`

**Query Parameters**:
- `date` (required, string): `YYYY-MM-DD`. An invalid date returns HTTP `400`.

**Response**:
```json
{
  "status": "ok",
  "data": {
    "days": [ { "date": "2026-02-02", "distance_nm": 24.3, "sailing_distance_nm": 20.0, "motoring_distance_nm": 4.3 } ],
    "min_distance": 0.0,
    "max_distance": 48.0,
    "total_distance": 310.5,
    "total_sailing_distance": 250.0,
    "total_motoring_distance": 60.5
  },
  "error": null
}
```

---

## Navigation and Environment

### Get Environmental Metrics

**Endpoint**: `GET /api/metrics`

**Query Parameters**:
- `metric` (required, string): Metric ID
- `trip_id` (optional, integer)
- `start` / `end` (optional, string)
- `max_points` (optional, integer)

**Metric IDs**:
- `1` Atmospheric Pressure (Pa)
- `2` Cabin Temperature (°C)
- `3` Water Temperature (°C)
- `4` Humidity (%)
- `5` Wind Speed (knots)
- `6` Wind Direction (degrees)
- `7` Roll (degrees)

**Example**:
```bash
curl "http://localhost:8080/api/metrics?metric=2&trip_id=132"
```

**Response** (`data` is an array):
```json
{ "timestamp": "2026-02-01T08:00:00.000Z", "metric_id": "2", "avg_value": 18.2, "max_value": 18.6, "min_value": 17.9 }
```

---

### Get Environmental Metrics (batch)

Several metrics in one request.

**Endpoint**: `GET /api/metrics/batch`

**Query Parameters**:
- `metrics` (required, string): Comma-separated metric IDs, e.g. `1,2,4`. An invalid or empty list returns `"status": "error"`.
- `trip_id`, `start`, `end`, `max_points`: as in [Get Environmental Metrics](#get-environmental-metrics)

**Response**: `data.metrics` maps each metric ID to an array of points in the same shape as above.
```json
{ "status": "ok", "data": { "metrics": { "1": [ { "timestamp": "...", "metric_id": "1", "avg_value": 101300.0, "max_value": 101320.0, "min_value": 101280.0 } ], "2": [] } }, "error": null }
```

---

### Get Navigation Analysis

Per-leg navigation-window analysis (how much of each leg was trimmed as marina exit/entry).

**Endpoint**: `GET /api/nav_analysis`

**Query Parameters**: `trip_id` (optional, integer): limit to one trip

**Response** (`data` is an array):
```json
{
  "trip_id": 132, "leg_number": 1,
  "leg_start": "...", "leg_end": "...", "leg_duration_ms": 28800000,
  "nav_start": "...", "nav_end": "...", "nav_detection_method": "engine_transition",
  "trimmed_start_ms": 1200000, "trimmed_end_ms": 1200000,
  "has_override": false
}
```

---

### Get AIS Targets

Recently seen AIS targets from the in-memory cache.

**Endpoint**: `GET /api/ais_targets`

**Response** (`data` is an array):
```json
{
  "mmsi": 247123456, "name": "EXAMPLE", "callsign": "IABC", "ship_type": 36, "ais_class": "A",
  "latitude": 45.1, "longitude": 12.2,
  "sog": 3.1, "cog": 1.57, "heading": 1.55,
  "nav_status": "Under way using engine",
  "last_seen": 1770000000000
}
```
`sog` is in m/s, `cog` and `heading` are in radians, `last_seen` is epoch milliseconds. All fields except `mmsi` and `last_seen` can be `null`.

---

## Trip Editing

All endpoints in this section are **[write]**.

### Update Trip Description

**Endpoint**: `POST /api/trip_description`

**Request Body** (JSON): `{ "id": 132, "description": "Weekend sail" }`

**Response**: `{ "status": "ok", "data": null, "error": null }`

---

### Delete Trip

**Endpoint**: `DELETE /api/delete_trip?id=<trip_id>`

Deletes the trip and all associated data.

---

### Trim Trip

Removes waypoints from the start and end of a trip around mooring.

**Endpoint**: `POST /api/trim_trip?id=<trip_id>`

---

### Invalidate Trip Legs Cache

Forces trip legs to be recomputed on the next [Get Trip Legs](#get-trip-legs).

**Endpoint**: `POST /api/invalidate_trip_legs?id=<trip_id>`

---

### Correct Engine Status

Overwrites the engine state for a time range of a trip.

**Endpoint**: `POST /api/correct_engine_status`

**Request Body** (JSON):
```json
{ "trip_id": 132, "start_timestamp": "2026-02-01T09:00:00Z", "end_timestamp": "2026-02-01T09:30:00Z", "engine_on": true }
```

---

### Fix Mooring Status

Sets `is_moored` for all vessel status rows in a time range, resampling to the moored reporting interval where needed.

**Endpoint**: `POST /api/fix_mooring_status`

**Request Body** (JSON):
```json
{ "start_timestamp": "2026-02-01T09:00:00Z", "end_timestamp": "2026-02-01T09:30:00Z", "is_moored": true }
```

**Response**:
```json
{ "status": "ok", "data": { "trip_id": 132, "rows_matched": 60, "rows_after": 4, "resampled": true }, "error": null }
```

---

### Set Navigation Window Override

Overrides the detected navigation start/end of a single leg. Note: this endpoint uses `PUT`, which the configured CORS layer does not allow cross-origin.

**Endpoint**: `PUT /api/nav_window?trip_id=<id>&leg_number=<n>`

**Request Body** (JSON, both fields optional):
```json
{ "nav_start": "2026-02-01T08:30:00Z", "nav_end": "2026-02-01T15:30:00Z" }
```

---

### Export Trip

Writes a trip to a JSON file on the server.

**Endpoint**: `GET /api/export_trip` (available in read-only mode too)

**Query Parameters**:
- `id` (required, integer)
- `path` (optional, string): relative path, default `static/exports/trip_{id}.json`. Absolute paths, `..` and `\` are rejected.

**Response**: `"data": "Trip 132 exported to static/exports/trip_132.json"`

---

### Import Trip

**Endpoint**: `POST /api/import_trip` **[write]**

**Content-Type**: `multipart/form-data`, field `file` containing the exported trip JSON (upload size limit applies).

```bash
curl -X POST -F "file=@trip_132.json" http://localhost:8080/api/import_trip
```

**Response**: `"data"` is a human-readable confirmation string; on failure `"status": "error"` (`"No file uploaded"` if the `file` field is missing).

---

### List Exports

**Endpoint**: `GET /api/list_exports` **[write]**

**Response** (`data` is an array, newest name first, `.json` files in `static/exports` only):
```json
{ "name": "trip_132.json", "size": 123456, "modified": "2026-02-03 10:00:00 UTC" }
```

---

## Runtime Toggles

All toggle endpoints are **[write]**, are persisted in the `system_status` table, and use the same shapes:

- `GET` returns `{ "status": "ok", "data": { "enabled": <bool> }, "error": null }`
- `POST` takes `{ "enabled": <bool> }` and returns the stored value in the same shape.

| Path | Meaning |
|---|---|
| `/api/tracking/status` | Position tracking: record vessel status reports and trips |
| `/api/metrics/status` | Environmental metrics (wind, pressure, temperatures, humidity) recording |
| `/api/auto_on/status` | **Auto On**: re-enable tracking and metrics when the vessel leaves its mooring |
| `/api/signalk/status` | SignalK WebSocket broadcasting (also needs `signalk.enabled` in config) |
| `/api/udp_broadcast/status` | UDP broadcasting (also needs `udp.enabled` in config) |

A key that has never been stored reads as enabled. `tracking_enabled` and `metrics_enabled` are seeded on in `schema.sql`; `auto_on_enabled` is seeded **off** (on connect, if missing), so Auto On is opt-in.

```bash
curl -X POST http://localhost:8080/api/tracking/status \
  -H "Content-Type: application/json" -d '{"enabled": false}'
```

### Auto On

While Auto On is enabled the router keeps analysing the NMEA stream even when position tracking is off, so it can detect a mooring departure. When the vessel transitions from moored to moving, `tracking_enabled` and `metrics_enabled` are both set to `true` (each only if currently off) and the change is logged. Auto On never disables tracking. Nothing is recorded while tracking is off; only the detection runs.

---

## Backups and System

All endpoints in this section are **[write]**.

### Create Backup

**Endpoint**: `POST /api/backup`

Runs `scripts/backup.sh` and stores a `backup_YYYYMMDD_HHMMSS.gz` under `backups/`. Only one backup can run at a time (a concurrent request returns `"status": "error"`).

**Response**: `{ "status": "ok", "data": { "file": "backup_20260203_100000.gz" }, "error": null }`

### List Backups

**Endpoint**: `GET /api/backup`

**Response** (`data` is an array, newest name first): `{ "name": "backup_20260203_100000.gz", "size": 1234567, "modified": "2026-02-03 10:00:00 UTC" }`

### Download Backup

**Endpoint**: `GET /api/backup/download?file=<name>`

Returns the file as `application/gzip` with a `Content-Disposition: attachment` header (not the JSON envelope). `400` if the name contains `/`, `\` or `..`; `404` if missing.

### Delete Backup

**Endpoint**: `DELETE /api/backup?file=<name>` or `DELETE /api/backup?all=true`

### System Shutdown

**Endpoint**: `POST /api/system/shutdown`

Runs `./shutdown.sh` if present, otherwise `systemctl poweroff`, about 500 ms after responding with `"data": "Shutdown initiated"`.

---

## Health and Configuration

### Health

**Endpoint**: `GET /api/health` (public, no authentication)

Returns HTTP `200` when healthy and `503` when degraded or down, so `curl -f` and uptime monitors work. The body is **not** wrapped in the envelope:

```json
{
  "enabled": true,
  "status": "ok",
  "can": "...",
  "alarms": [ { "id": "...", "severity": "warn", "since": "...", "message": "..." } ],
  "streams": { "<stream>": 1200 },
  "db": { "<name>": { "consecutive_failures": 0, "last_success_age_ms": 800 } },
  "loop": { "heartbeat_age_ms": 40, "last_slow_ms": null, "last_slow_age_ms": null }
}
```
`status` is `ok`, `degraded` or `down`; `severity` is `warn` or `alarm`; `streams` maps each tracked data stream to the age in ms of its last message (`null` if never seen).

### Read-only Flag

**Endpoint**: `GET /api/config/read_only` → `{ "read_only": false }` (no envelope)

### Capabilities

**Endpoint**: `GET /api/config/capabilities` → `{ "udp_broadcast": true }` (no envelope). Tells the UI which toggles are meaningful given the static config.

---

## Sync

Used to mirror trips from the boat to a remote instance. `GET /api/sync/status` and `POST /api/sync/push` are for the boat (the latter is **[write]**); `POST /api/sync/manifest` and `POST /api/sync/trip` are called by the boat on the remote.

### Sync Status

**Endpoint**: `GET /api/sync/status`

**Response**: `{ "status": "ok", "data": { "last_synced_at": "2026-02-03T10:00:00+00:00", "push_enabled": true }, "error": null }` (`last_synced_at` can be `null`).

### Push to Remote

**Endpoint**: `POST /api/sync/push[?dry_run=true]` **[write]**

Requires `sync.enabled`, `sync.target_url` and `sync.api_key` in config. With `dry_run=true` nothing is written or deleted on the remote.

**Response** (`data`): `deleted_count`, `upserted_count`, `synced_at`, `dry_run`, and, for dry runs only, `to_add` and `to_update` (arrays of `{uuid, description, start, end}`) plus the trips the remote would delete.

### Receive Manifest (remote side)

**Endpoint**: `POST /api/sync/manifest`

**Authentication**: `Authorization: Bearer <sync.api_key>` (the session cookie is not used). A missing or wrong token is rejected.

**Request Body**: `{ "trip_versions": { "<uuid>": <version>, ... }, "dry_run": false }`

Deletes trips not listed in the manifest (unless `dry_run`) and returns the UUIDs the sender should push.

**Response** (`data`): `{ "deleted_count": 0, "uuids_to_push": ["..."], "to_add": [], "to_update": [], "to_delete": [] }`. `to_add`, `to_update` and `to_delete` are only filled in dry-run mode.

### Receive Trip (remote side)

**Endpoint**: `POST /api/sync/trip`

**Authentication**: bearer token as above. **Request Body**: exported trip JSON. Imports (or replaces) the trip. Up to 100 MiB.

---

## Forecast

Weather forecast areas, model data and route planning. All forecast endpoints are **[write]** (not registered in read-only mode).

### List Areas

`GET /api/forecast/areas` → `data`: array of `{ "id", "lat_min", "lat_max", "lon_min", "lon_max", "created_at" }`.

### Create Area

`POST /api/forecast/areas` with `{ "lat_min": 44.0, "lat_max": 46.0, "lon_min": 12.0, "lon_max": 14.0 }` → `data`: the new area ID.

### Delete Area

`DELETE /api/forecast/areas?id=<id>`. Returns HTTP `404` if the area does not exist.

### Forecast Status

`GET /api/forecast/status` → `{ "online": true, "last_fetch": "2026-02-03T09:00:00Z", "next_fetch": "2026-02-03T12:00:00Z", "area_count": 1, "point_count": 240 }` (`last_fetch`/`next_fetch` can be `null`).

### Refresh Forecast

`POST /api/forecast/refresh` fetches all areas now. `data` is a string like `"240 grid points fetched"`; an error is returned if there are no areas or the fetch fails.

### Grid Points

`GET /api/forecast/grid-points?timestamp=<ts>` → array of forecast values at that time:
```json
{ "lat": 45.0, "lon": 13.0, "wind_speed_kn": 12.0, "wind_direction_deg": 270.0, "wind_gust_kn": 18.0,
  "wave_height_m": 0.8, "wave_period_s": 5.0, "wave_direction_deg": 260.0, "cape_j_kg": 0.0, "model": "..." }
```
All value fields can be `null`.

### Route Overlay

Forecast conditions along a route you define.

`GET /api/forecast/route`

**Query Parameters**:
- `waypoints` (required): `lat1,lon1;lat2,lon2;...` (at least two points)
- `departure` (required): RFC 3339 timestamp (e.g. `2026-06-01T06:00:00Z`)
- `motoring_speed_kn` (required, > 0)
- `polar_efficiency` (optional, default `1.0`): fraction of polar speed used (0–1)
- `min_sail_speed_kn` (optional, default `0`): motor when sail speed is below this
- `min_twa_deg` (optional, default `60`, range 0–180): motor when closer to the wind than this

**Response** (`data` is an array):
```json
{ "lat": 45.0, "lon": 13.0, "timestamp": "2026-06-01T06:00:00Z",
  "wind_speed_kn": 12.0, "wind_direction_deg": 270.0, "wind_gust_kn": 18.0,
  "wave_height_m": 0.8, "wave_period_s": 5.0, "wave_direction_deg": 260.0, "cape_j_kg": 0.0,
  "speed_kn": 5.5, "twa_deg": 95.0, "wind_model": "...", "relative_wind_deg": 95.0, "heading_deg": 180.0 }
```

### Optimal Route

Isochrone routing over the forecast. Requires a configured polar table and available forecast data.

`GET /api/forecast/optimal-route`

**Query Parameters**: `from_lat`, `from_lon`, `to_lat`, `to_lon`, `departure` (RFC 3339), `motoring_speed_kn` (> 0), and optional `polar_efficiency`, `min_sail_speed_kn`, `min_twa_deg` (as above) and `sail_weight_kn` (default `0`, must be ≥ 0).

**Response**: `data` is `{ "route": [ <route overlay points> ], "frontiers": [ [ { "lat", "lon", "parent_idx", "speed_kn", "motoring", "wind_speed_kn", "wind_dir_deg", "wind_gust_kn", ... } ] ] }`.

---

## Authentication Endpoints

Always public. Only meaningful if `web.auth_password` is set.

- `POST /api/auth/login` with `{ "password": "..." }` → `200` and a `session` cookie (`HttpOnly`, `SameSite=Strict`, `Secure` if `web.secure_cookies`) valid for `web.session_duration_secs`; `401` with `{"status":"error","error":"Invalid password"}` on mismatch.
- `POST /api/auth/logout` → clears the cookie.
- `GET /api/auth/status` → `{ "auth_required": true }`.

---

## Error Handling

### HTTP Status Codes

- `200`: Success, or an application error described by `"status": "error"`
- `400`: Invalid parameter (bad date format, invalid backup file name)
- `401`: Authentication required
- `404`: Unknown route (including write endpoints in read-only mode), or missing resource where noted
- `413`: Upload larger than the allowed size (100 MiB for trip import and sync)
- `500`: Internal error (also returned for a few database failures on toggle and forecast-area writes)
- `503`: `GET /api/health` when degraded or down

### Common Error Scenarios

- Invalid date/time: HTTP `400`. Use RFC 3339 with timezone or `YYYY-MM-DD HH:MM:SS`.
- Missing required parameter: HTTP `400` from the query parser.
- Unknown trip ID: `"status": "error"`, `"error": "Trip <id> not found"`.
- Database failure: `"status": "error"` with the database error message.

---

## Usage Examples

### JavaScript (Fetch API)

```javascript
const res = await fetch('/api/trips?last_months=3');
const { status, data, error } = await res.json();
if (status === 'ok') console.log(data);

await fetch('/api/trip_description', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ id: 132, description: 'Weekend sail' })
});
```

### Python (requests)

```python
import requests
r = requests.get('http://localhost:8080/api/metrics',
                 params={'metric': 1, 'start': '2026-02-01T00:00:00Z', 'end': '2026-02-03T23:59:59Z'})
print(r.json()['data'])
```

### cURL

```bash
curl "http://localhost:8080/api/trips?last_months=3" | jq .
curl "http://localhost:8080/api/track?trip_id=132" | jq '.data[] | {time: .timestamp, lat: .latitude, lon: .longitude}'
curl "http://localhost:8080/api/metrics?metric=1&start=2026-02-01T00:00:00Z&end=2026-02-03T23:59:59Z" | jq .
curl -X POST http://localhost:8080/api/auto_on/status -H "Content-Type: application/json" -d '{"enabled": true}'
```

---

## Data Aggregation

### Track Points
Track points follow the reporting intervals in `config.json` (`database.vessel_status`): the underway interval while moving and `interval_moored_seconds` while moored.

### Environmental Metrics
Metrics are collected continuously and stored as aggregates with min, max and average per metric-specific interval.

---

## Configuration

The web server section of `config.json`:

```json
{
  "web": {
    "enabled": true,
    "port": 8080,
    "auth_password": null,
    "session_duration_secs": 604800,
    "secure_cookies": true,
    "read_only": false
  }
}
```

- `enabled`: enable the web server (default `true`)
- `port`: TCP port (default `8080`)
- `auth_password`: shared UI password; unset or empty disables authentication
- `session_duration_secs`: session lifetime (default 7 days)
- `secure_cookies`: set the `Secure` flag on the session cookie (default `true`; disable for plain-HTTP local use)
- `read_only`: expose only read endpoints (default `false`)

See `config.example.json` for all options.

---

## CORS and Security

The server sends permissive CORS headers (any origin; methods `GET`, `POST`, `DELETE`; headers `Content-Type`, `Cookie`). Authentication is optional and uses a shared password (see [Authentication](#authentication)); there is no per-user access control, rate limiting or built-in TLS. For internet exposure use `web.read_only`, a password, `secure_cookies` and a TLS-terminating reverse proxy.

---

## Web Interface

Static files are served from the `static/` directory (for example `index.html`, `trip.html`, `plan.html`, `realtime.html`, `ais.html`, `backup.html`, `compass.html`, `yearly-stats.html`, `navigation-areas.html`, `fix-engine-status.html`, `signalk-browser.html`, `login.html`). When authentication is enabled, unauthenticated page requests are redirected to `/login.html`. The SignalK WebSocket stream is served at `/signalk/v1/stream`.

---

## Troubleshooting

### Server Not Starting
- Check if the port is already in use
- Verify `web.enabled` is `true` in `config.json`
- Check application logs for database connection issues

### Empty Data Responses
- Verify the database contains data for the requested parameters
- Check the date/time format (RFC 3339 with timezone, or `YYYY-MM-DD HH:MM:SS`)
- Ensure trip IDs are valid

### Connection Refused
- Confirm the NMEA Router application is running
- Verify firewall settings allow connections to the configured port

---

For more information see:
- `schema.sql` - Database structure
- `docs/ENVIRONMENTAL_MONITORING.md` - Environmental metrics details
- `docs/README_DATABASE.md` - Database implementation details
- `docs/SIGNALK_BROADCAST_CONTROL.md` - SignalK/UDP broadcast toggles
