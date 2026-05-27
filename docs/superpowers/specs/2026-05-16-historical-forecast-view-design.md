# Historical Forecast View — Design Spec

**Date:** 2026-05-16  
**Status:** Approved

## Summary

Enable `plan.html` to be opened for historical (completed) trips, so the stored forecast data can be reviewed on the interactive wind-arrow map. The vessel's GPS position is shown as a marker that moves with the time slider.

No backend changes are required. All needed APIs already exist.

---

## Changes

### 1 — trip.html: Unlock the Planning button for historical trips

**File:** `static/trip.html`, line ~3013

**Current behaviour:** Planning button is hidden when `isActive` is false (trip ended > 2 hours ago), regardless of whether forecast data exists.

**New behaviour:** Show the button whenever `point_count > 0`, regardless of `isActive`. The `point_count` check is the correct gate — no data, no button.

**Change:** Two small edits:

a) In `initForecastAreasSection`, remove the `isActive` gate on `planBtn.style.display`. Only set `planBtn.href`; leave visibility control to `updateForecastStatus`. The button starts `display:none` in the HTML and the status call shows it when appropriate.

b) In `updateForecastStatus()`, after receiving the status response, show or hide the button based on `point_count`:
```js
const planBtn = document.getElementById('forecastPlanningBtn');
if (planBtn) planBtn.style.display = (s.point_count > 0) ? '' : 'none';
```

`updateForecastStatus` is already called at the end of `initForecastAreasSection` and every 60 seconds thereafter, so `point_count` is the sole authoritative gate with no visible flash.

---

### 2 — plan.html: Day tabs scoped to trip dates

**Current behaviour:** `loadAvailableDays()` generates 7 tabs from today → today+6 UTC days.

**New behaviour:** Fetch `/api/trip?id=X` on init to get `start_date` and `end_date`. Generate tabs from the trip's first UTC day through `end_date + 7 days` (inclusive). For active trips (trip `end_date` within 2 hours of now) the range still ends with 7 forecast days beyond the current end.

**Implementation:**

```js
async function loadTripInfo() {
    const resp = await fetch('/api/trip?id=' + tripId);
    const json = await resp.json();
    return json.data;   // { start_date, end_date, ... }
}
```

In `loadAvailableDays()`:
1. Fetch trip info.
2. Compute `firstDay` = UTC midnight of `start_date`.
3. Compute `lastDay` = UTC midnight of `end_date` + 7 days.
4. Build `availableDays` array, one entry per UTC day from `firstDay` to `lastDay`.

**Default selected day on load:**
- Active trip (end within 2 hours): `jumpToNow()` (today's tab, current hour).
- Historical trip: `selectDay(0)` → trip start.

**Plan Route button visibility:**
- Active trip: shown (existing behaviour).
- Historical trip: hidden. Set `planRouteBtn.style.display = 'none'` during init.

---

### 3 — plan.html: Vessel position marker

**New state variables:**
```js
let tripTrack = [];          // [{lat, lon, timestamp}, ...] sorted by time
let boatMarker = null;       // single L.circleMarker, reused
let tripStart = null;        // Date object
let tripEnd = null;          // Date object
```

**On init:** Fetch the trip track once, capped at 2000 points (sufficient for hourly slider resolution):
```js
const trackResp = await fetch('/api/track?trip_id=' + tripId + '&max_points=2000');
const trackJson = await trackResp.json();
tripTrack = trackJson.data || [];
```

**On every slider change** (alongside `loadGridPoints`, same 150ms debounce):
- Call `updateBoatMarker()`.

**`updateBoatMarker()`:**
1. Get `selectedTime = new Date(getSelectedISO())`.
2. If `selectedTime < tripStart || selectedTime > tripEnd`: remove marker and return.
3. Binary-search `tripTrack` for the point with timestamp closest to `selectedTime`.
4. If `boatMarker` is null: create a `L.circleMarker` with a distinct style (e.g. blue fill, white stroke, radius 8).
5. Move `boatMarker` to the found position (`boatMarker.setLatLng([lat, lon])`).
6. Bind a tooltip showing the timestamp.

The marker is never recreated — only moved or shown/hidden via `addTo`/`removeFrom` the map.

---

## Data flow

```
init()
  └── /api/trip?id=X          → tripStart, tripEnd, isActive
  └── /api/forecast/status    → point_count (gate check, abort if 0)
  └── /api/track?trip_id=X   → tripTrack[]
  └── buildDayTabs()
  └── selectDay(0) or jumpToNow()

slider change (debounced 150ms)
  └── loadGridPoints()        → /api/forecast/grid-points → wind arrows
  └── updateBoatMarker()      → binary search tripTrack → circleMarker position
```

---

## Edge cases

- **No forecast data:** `point_count == 0` → `loadAvailableDays` shows the existing "No forecast data" message; no tabs, no marker fetch.
- **No track data:** `tripTrack` is empty → `updateBoatMarker` exits immediately; no marker shown.
- **Very long trips:** day tabs can be many entries. Tabs wrap (existing `flex-wrap` on the container handles this).
- **Active trip:** `end_date` in the DB may lag slightly behind real time; the 2-hour window in `isActive` detection already accounts for this.

---

## Files changed

| File | Change |
|---|---|
| `static/trip.html` | Remove `isActive` gate on Planning button |
| `static/plan.html` | Fetch trip info, scope day tabs, load track, vessel marker |

No Rust changes. No schema changes.
