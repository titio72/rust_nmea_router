# Isochrone Frontier Darker Styling + Forecast-Area Clipping — Design

## Purpose

Follow-up to the isochrone frontier visualization feature
([2026-07-05-isochrone-frontier-visualization-design.md](2026-07-05-isochrone-frontier-visualization-design.md)).
Two changes to `drawFrontiers()` in `static/plan.html`:

1. The frontier polylines are too faint to read (`#888` at `opacity: 0.35`) — darken them.
2. Frontier lines currently draw across the whole map regardless of forecast coverage. Restrict
   them to only the portions that fall within a configured forecast area, since a frontier point
   outside all forecast areas has no wind data backing its "how far the boat could get" claim.

## Scope

- Applies only to `drawFrontiers()` and its immediate helpers in `static/plan.html`.
- No backend changes — `IsochroneResult.frontiers` and the `/api/forecast/optimal-route` response
  shape are unchanged. This is purely a client-side rendering restriction.
- No change to forecast area management (`ForecastArea`, `/api/forecast/areas`) — areas remain
  simple lat/lon bounding boxes (`lat_min`, `lat_max`, `lon_min`, `lon_max`).

## Styling change

In `drawFrontiers()`, change the polyline style from:
```js
{ color: '#888', weight: 1, opacity: 0.35 }
```
to:
```js
{ color: '#444', weight: 1, opacity: 0.6 }
```
Weight is unchanged — only darkness/opacity was raised as a concern.

## Forecast-area clipping

Forecast areas (`planAreas`, already loaded client-side for the wind overlay) are rectangular
lat/lon boxes. `showWindPopupAt()` (`static/plan.html:586-588`) already tests point-in-area with:
```js
planAreas.find(a =>
    latlng.lat >= a.lat_min && latlng.lat <= a.lat_max &&
    latlng.lng >= a.lon_min && latlng.lng <= a.lon_max);
```

Extract the box-containment test into a shared helper:
```js
function pointInAnyArea(lat, lon) {
    return planAreas.some(a =>
        lat >= a.lat_min && lat <= a.lat_max &&
        lon >= a.lon_min && lon <= a.lon_max);
}
```
`showWindPopupAt()` is updated to use this helper too (via `planAreas.find(a => ...)` calling the
same per-area predicate, extracted as `pointInArea(lat, lon, area)` so both the `.find` and
`.some` call sites share one containment expression) — a small DRY cleanup enabled by touching
this logic, not a new feature.

`drawFrontiers()` changes from "skip the whole frontier if it has fewer than 2 points" to:
walk each frontier's points in order, splitting it into contiguous runs of points that are inside
at least one forecast area (via `pointInAnyArea`), and draw one polyline per run of length ≥ 2 —
matching today's existing minimum-length skip, just applied per-run instead of per-frontier. A
run of a single in-area point (surrounded by out-of-area points) still draws nothing, same as
today's behavior for a 1-point frontier.

**Known consequence:** if `planAreas` is empty (no forecast areas configured), `pointInAnyArea`
always returns `false`, so no frontier lines draw at all — confirmed as the intended behavior for
this design, not a bug.

## Testing

No automated test suite exists for `plan.html` (plain JS, no build step) — consistent with the
prior isochrone visualization feature. Verification is a manual browser check:
1. Configure at least one forecast area covering part of a planned route.
2. Run "Optimize" on a route whose isochrone search extends both inside and outside that area.
3. Confirm frontier lines only appear inside the forecast area's bounding box, are visibly darker
   than before, and a frontier that exits and re-enters the area renders as separate line segments
   rather than one continuous line jumping across the gap.
4. Confirm `showWindPopupAt()` (clicking the map for a wind popup) still works correctly after the
   `pointInArea` extraction.
