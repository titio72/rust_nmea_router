# Historical Forecast View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `plan.html` to be opened for historical (completed) trips, showing stored forecast wind arrows with a vessel position marker that tracks the time slider.

**Architecture:** Pure frontend change — two HTML files modified, no Rust/backend changes. `trip.html` unlocks the Planning button via a `point_count`-based gate in the status poller. `plan.html` fetches trip info to scope day tabs to the trip's date range, loads the track once at init, and maintains a single Leaflet marker that moves with the time slider.

**Tech Stack:** Vanilla JS, Leaflet 1.9.4, existing REST API (`/api/trip`, `/api/track`, `/api/forecast/*`)

---

## File Map

| File | What changes |
|---|---|
| `static/trip.html` | Remove `isActive` gate on Planning button; add `point_count` gate to `updateForecastStatus()` |
| `static/plan.html` | Fetch trip info on init; scope day tabs to trip date range; load track; vessel position marker |

---

### Task 1: trip.html — gate Planning button on `point_count`, not `isActive`

**Files:**
- Modify: `static/trip.html` (lines ~3011–3014 and ~2980–2996)

- [ ] **Step 1: Remove the `isActive` gate on the Planning button**

In `initForecastAreasSection()`, find this block (~line 3011):
```js
const planBtn = document.getElementById('forecastPlanningBtn');
if (planBtn) {
    planBtn.style.display = isActive ? '' : 'none';
    planBtn.href = 'plan.html?id=' + tripId;
}
```
Replace it with:
```js
const planBtn = document.getElementById('forecastPlanningBtn');
if (planBtn) {
    planBtn.href = 'plan.html?id=' + tripId;
}
```
The button starts `display:none` in the HTML; `updateForecastStatus` will control visibility going forward.

- [ ] **Step 2: Show/hide the Planning button in `updateForecastStatus`**

In `updateForecastStatus()`, after `const s = json.data;` (line 2982), add:
```js
const planBtn = document.getElementById('forecastPlanningBtn');
if (planBtn) planBtn.style.display = (s && s.point_count > 0) ? '' : 'none';
```

`s` is already `json.data` (line 2982 in the existing code), so `s.point_count` is the correct path.

- [ ] **Step 3: Verify in the browser**

Open `trip.html?id=<historical-trip-id>` for a trip that has forecast data. Scroll to the Forecast Areas section. Confirm the Planning button is now visible. Click it — `plan.html?id=<id>` should open.

Open `trip.html?id=<trip-id-with-no-forecast>`. Confirm the Planning button does not appear.

- [ ] **Step 4: Commit**

```bash
git add static/trip.html
git commit -m "feat: show Planning button for historical trips with forecast data"
```

---

### Task 2: plan.html — fetch trip info and scope day tabs to trip date range

**Files:**
- Modify: `static/plan.html`

The current `loadAvailableDays()` builds 7 tabs relative to today. Replace it with a version that fetches the trip's `start_date` / `end_date` from `/api/trip?id=X` and generates tabs from trip start through trip end + 7 days.

- [ ] **Step 1: Add state variables for trip info**

Near the top of the `<script>` block, after the existing state declarations (`let planMap = null; ...`), add:
```js
let tripInfo = null;      // raw TripSummary from /api/trip
let tripStart = null;     // Date object — UTC midnight of trip start day
let tripEnd = null;       // Date object — trip end_date parsed as Date
let tripTrack = [];       // [{latitude, longitude, timestamp}, ...] sorted by time
let boatMarker = null;    // single L.circleMarker reused across slider moves
```

- [ ] **Step 2: Add `loadTripInfo()` helper**

After the state variables, add:
```js
async function loadTripInfo() {
    const resp = await fetch('/api/trip?id=' + tripId);
    const json = await resp.json();
    if (!json.data) return null;
    return json.data;   // { start_date, end_date, description, ... }
}
```

- [ ] **Step 3: Rewrite `loadAvailableDays()` to use trip date range**

Replace the current `loadAvailableDays()` function with:
```js
async function loadAvailableDays() {
    try {
        // Check forecast data exists
        const statusResp = await fetch('/api/forecast/status?trip_id=' + tripId);
        const statusJson = await statusResp.json();
        if (!statusJson.data || statusJson.data.point_count === 0) {
            document.getElementById('dayTabsContainer').innerHTML =
                '<span style="font-size:13px; color:var(--text-secondary);">' +
                'No forecast data. Add areas on the trip page first.</span>';
            return;
        }

        // Fetch trip date range
        tripInfo = await loadTripInfo();
        if (!tripInfo) {
            document.getElementById('dayTabsContainer').innerHTML =
                '<span style="font-size:13px; color:var(--text-secondary);">Trip not found.</span>';
            return;
        }

        // Parse trip bounds
        tripStart = new Date(tripInfo.start_date);
        tripEnd = new Date(tripInfo.end_date);

        // Build day tabs: UTC midnight of start_date through UTC midnight of (end_date + 7 days)
        const firstDay = new Date(tripStart);
        firstDay.setUTCHours(0, 0, 0, 0);

        const lastDay = new Date(tripEnd);
        lastDay.setUTCHours(0, 0, 0, 0);
        lastDay.setUTCDate(lastDay.getUTCDate() + 7);

        availableDays = [];
        const cur = new Date(firstDay);
        while (cur <= lastDay) {
            availableDays.push({
                date: new Date(cur),
                label: cur.toLocaleDateString('en-GB', {
                    weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC'
                })
            });
            cur.setUTCDate(cur.getUTCDate() + 1);
        }

        renderDayTabs();

        // Active trip: end within 2 hours of now → jump to current time
        const isActive = (Date.now() - tripEnd.getTime()) < 7_200_000;
        if (isActive) {
            jumpToNow();
        } else {
            selectDay(0, true);
        }

        // Hide "Plan Route" for historical trips
        if (!isActive) {
            const btn = document.getElementById('planRouteBtn');
            if (btn) btn.style.display = 'none';
        }

    } catch (err) {
        console.error('Failed to load available days', err);
    }
}
```

- [ ] **Step 4: Verify day tabs in browser**

Open `plan.html?id=<historical-trip-id>`. Confirm:
- Day tabs start at the trip's start date, not today
- Last tab is 7 days after the trip's end date
- "Plan Route" button is not visible
- Selecting any tab and scrubbing the slider loads wind arrows (if forecast data was stored for that period)

Open `plan.html?id=<active-trip-id>`. Confirm:
- Tabs span from trip start to now + 7 days
- "Plan Route" button is visible
- Slider defaults to current time

- [ ] **Step 5: Commit**

```bash
git add static/plan.html
git commit -m "feat: scope plan.html day tabs to trip date range"
```

---

### Task 3: plan.html — vessel position marker

**Files:**
- Modify: `static/plan.html`

Load the trip's GPS track at init and show a marker at the interpolated position as the slider moves. The marker is hidden when the selected time falls outside the trip's actual timespan.

- [ ] **Step 1: Add `loadTripTrack()` function**

After `loadTripInfo()`, add:
```js
async function loadTripTrack() {
    if (!tripId) return;
    try {
        const resp = await fetch('/api/track?trip_id=' + tripId + '&max_points=2000');
        const json = await resp.json();
        tripTrack = (json.data || []).filter(p => p.latitude != null && p.longitude != null);
    } catch (err) {
        console.error('Failed to load trip track', err);
        tripTrack = [];
    }
}
```

- [ ] **Step 2: Call `loadTripTrack()` from `init()`**

In `init()`, after `await loadAvailableDays();`, add:
```js
await loadTripTrack();
```

- [ ] **Step 3: Add `updateBoatMarker()` function**

After `loadTripTrack()`, add:
```js
function updateBoatMarker() {
    const ts = getSelectedISO();
    if (!ts || !tripTrack.length || !tripStart || !tripEnd) {
        if (boatMarker) { planMap.removeLayer(boatMarker); boatMarker = null; }
        return;
    }
    const t = new Date(ts).getTime();
    if (t < tripStart.getTime() || t > tripEnd.getTime()) {
        if (boatMarker) { planMap.removeLayer(boatMarker); boatMarker = null; }
        return;
    }

    // Binary search for nearest track point
    let lo = 0, hi = tripTrack.length - 1, best = 0;
    while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        const mt = new Date(tripTrack[mid].timestamp).getTime();
        const bestT = new Date(tripTrack[best].timestamp).getTime();
        if (Math.abs(mt - t) < Math.abs(bestT - t)) best = mid;
        if (mt < t) lo = mid + 1; else hi = mid - 1;
    }
    const pt = tripTrack[best];

    if (!boatMarker) {
        boatMarker = L.circleMarker([pt.latitude, pt.longitude], {
            color: '#fff', fillColor: '#3b82f6', fillOpacity: 1, radius: 8, weight: 2
        }).addTo(planMap);
    } else {
        boatMarker.setLatLng([pt.latitude, pt.longitude]);
        if (!planMap.hasLayer(boatMarker)) boatMarker.addTo(planMap);
    }
    boatMarker.bindTooltip(
        new Date(pt.timestamp).toLocaleTimeString('en-GB', {
            hour: '2-digit', minute: '2-digit', timeZone: 'UTC'
        }) + ' UTC',
        { permanent: false, direction: 'right' }
    );
}
```

- [ ] **Step 4: Call `updateBoatMarker()` alongside `loadGridPoints()`**

Find the debounce timer handler (existing code):
```js
hourDebounceTimer = setTimeout(loadGridPoints, 150);
```
Replace it with:
```js
hourDebounceTimer = setTimeout(() => { loadGridPoints(); updateBoatMarker(); }, 150);
```

Also call `updateBoatMarker()` in `selectDay()`, after `if (doLoad) loadGridPoints();`:
```js
function selectDay(i, doLoad = true) {
    selectedDay = i;
    selectedHour = 0;
    document.getElementById('hourSlider').value = 0;
    renderDayTabs();
    updateSelectedTime();
    if (doLoad) loadGridPoints();
    updateBoatMarker();
}
```

- [ ] **Step 5: Verify position marker in browser**

Open `plan.html?id=<historical-trip-id>`. Confirm:
- A blue circle marker appears on the map at the vessel's position when the slider is within the trip's timespan
- Scrubbing the slider moves the marker
- Selecting a day tab outside the trip's timespan hides the marker
- Hovering the marker shows a time tooltip
- Active trip: marker also moves correctly on scrub

- [ ] **Step 6: Commit**

```bash
git add static/plan.html
git commit -m "feat: show vessel position marker in plan.html for historical trips"
```
