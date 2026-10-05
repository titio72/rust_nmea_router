# Fastest-Segment Analytics: Fold Into Leg Cache — Design

## Purpose

`fetch_track_analytics` (backing `GET /api/track_analytics`, called from `trip.html`'s
`renderAnalytics()`) computes max speed and the fastest 1/5/10/25nm sailing segments by fetching
every raw `vessel_status` row in a time range and running `find_fastest_segment` — a nested loop
over all points — four times. For a 16-day trip (~32k points) this measured at ~0.5-1.9s per page
load, every page load, because nothing is cached. The nested loop is also worst-case O(n²): its
inner scan only breaks early once the target distance is covered, so a long becalmed-but-not-moored
stretch (which is exactly what tends to sit between legs, since legs are themselves separated by
mooring stops) can make one `start_idx` scan deep into the remaining array.

This change eliminates both problems by recognizing that a continuous sailing segment can never
span a mooring stop — so the fastest segment for an entire trip is just the best of the fastest
segments among its legs, and legs are already a semi-static, cached concept
(`trip_legs_cache`, see `fetch_trip_legs`/`compute_trip_legs` in `src/db/operations/query.rs`).

## Scope

- Extend the single vessel_status scan already performed by `compute_trip_legs` to also track,
  per leg: running max speed, and a proper single-pass fastest-segment search for 1/5/10/25nm.
- Extend `trip_legs_cache` (and the `TripLeg` struct) with the new fields. Same cache lifecycle as
  today: computed fresh while a trip is open, saved once closed, invalidated by the same three
  existing triggers (`delete_trip`, `trim_trip`, `correct_engine_status`).
- Delete `GET /api/track_analytics`, its handler, `fetch_track_analytics`, and the old
  `find_fastest_segment` outright — `trip.html` already fetches `/api/trip_legs` up front, so once
  legs carry this data the endpoint has no remaining purpose.
- Move `trip.html`'s `renderAnalyticsCards()` from async-fetch-then-render to a synchronous read of
  the already-loaded `currentLegs`.
- Out of scope: average-speed fields need no new storage or computation at all — they're derived
  at read time from data that already exists (see below). No changes to mooring detection, leg
  boundary detection itself, or `trip_legs_nav_overrides`.

## Algorithm

Within a leg, replace "for each start index, rescan forward" with a single monotonic two-pointer
pass per target distance:

1. Split the leg's points into maximal runs where `engine_on` is off — a run ends completely at
   any engine-on point, exactly matching today's semantics (a segment can never include a motoring
   point).
2. Within each run, for each of the four target distances, run a sliding window where `start` and
   `end` indices only ever advance (never reset backward): advance `end` until the cumulative
   distance from `start` to `end` reaches the target, record the candidate if so, then advance
   `start` and repeat. This is valid because consecutive-point distances are non-negative (the
   classic "minimum window with sum ≥ target" pattern), so `end` performs at most n total advances
   across the whole run — O(run length) per target distance, not O(run length²).
3. Track the best (highest average-speed) candidate per target distance across all runs in the leg.

Total work per leg is O(leg points), and legs partition the trip, so total work per trip is
O(trip points) — down from O(trip points²) worst case today. This also structurally removes the
pathological case: a becalmed stretch between two legs is never scanned by either leg's pass at
all, since it's mooring time and doesn't belong to a leg.

Semantics preserved exactly otherwise: same four target distances, same average-speed tiebreak,
same "no segment reported if sailing never continuously covers that distance." The one real
behavior change: a fastest segment can never span a mooring stop between legs. This tightens
correctness rather than losing capability — it was never actually possible to sail continuously
through a stop; today's implementation just didn't structurally prevent a becalmed-but-technically-
not-yet-moored gap from being included.

## Schema

New nullable columns on `trip_legs_cache`:

```
max_speed_kn              DOUBLE       NULL
max_speed_timestamp       VARCHAR(30)  NULL
fastest_1nm_distance_nm   DOUBLE       NULL
fastest_1nm_avg_speed_kn  DOUBLE       NULL
fastest_1nm_duration_ms   BIGINT UNSIGNED NULL
fastest_1nm_start_timestamp VARCHAR(30) NULL
fastest_1nm_end_timestamp   VARCHAR(30) NULL
-- ...repeated for fastest_5nm, fastest_10nm, fastest_25nm
```

22 new columns total, flat and typed to match the table's existing convention (not JSON), added
via the same best-effort `ALTER TABLE ... ADD COLUMN` pattern already used for the `nav_*` columns
in `get_cached_trip_legs`.

**Average speeds are not stored** — `average_speed_kn`/`average_speed_sailing_kn`/
`average_speed_motoring_kn` are derived at read time:
- Per leg: from `trip_legs_cache`'s existing `sailing_distance_nm`/`motoring_distance_nm`/
  `sailing_time_ms`/`motoring_time_ms` columns (already populated, already correct).
- Whole trip: from the `trips` row's own `total_distance_sailed`/`total_distance_motoring`/
  `total_time_sailing`/`total_time_motoring` columns (already incrementally maintained on every
  `vessel_status` insert in `src/db/operations/vessel_status.rs` — zero new computation).

**Migration for trips already cached:** no NULL-detection special-casing. A one-time
`DELETE FROM trip_legs_cache` after deploy makes every closed trip lazily recompute (and re-cache)
its legs — including the new fields — on its next visit. Same cost model as any other cache miss
today.

## Frontend (`static/trip.html`)

`renderAnalyticsCards()` changes from:
```js
renderAnalytics(startTime, endTime).catch(...)   // async fetch to /api/track_analytics
```
to a synchronous computation over the already-loaded `currentLegs`:
- **Full-trip view:** `max_speed_kn` = max across all legs (carrying that leg's timestamp); each
  `fastest_Xnm` = whichever leg has the best `avg_speed_kn` for that X, skipping legs where it's
  null; average speeds read from `currentTrip` (already fetched via `/api/trip`).
- **Single-leg view:** read that leg's fields directly from `currentLegs[i]`.

No network request, no loading state, no race with the rest of page load. `GET /api/track_analytics`
(route + handler in `src/web/api.rs`), `fetch_track_analytics`, and the old `find_fastest_segment`
are deleted, not deprecated — confirmed no other caller exists (only `trip.html`, `trip_timing.rs`,
and `query.rs` itself reference this endpoint today).

## Testing

- **Unit tests (no DB), TDD:** the new two-pointer segment-search function, in the same style as
  `decimate`'s tests (`src/db/operations/query.rs`) — correctness cases (segment found/not found,
  tiebreak on average speed, engine-on boundary respected) plus a regression test encoding the
  pathological case (a long near-zero-distance-but-not-moored run) asserting a bounded iteration
  count, not just correctness.
- **DB-backed `#[ignore]` tests:** extend existing `compute_trip_legs`/`fetch_trip_legs` coverage
  to assert the new fields populate correctly and are cached/invalidated exactly like the rest of
  `TripLeg`.
- **Frontend:** no JS test framework in this repo. Verify via the same headless-Chrome capture
  approach used earlier in this project — confirm `currentLegs` carries the new fields and
  `renderAnalyticsCards` fires zero additional network requests.
