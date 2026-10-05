// Distance-measurement tool for Leaflet maps: click once to drop the first point, click
// again to drop the second — draws a dashed line labelled with the distance (NM if >=1 NM,
// otherwise meters). A further click while two points are already set starts a new
// measurement. Distance uses the Haversine formula, per AGENTS.md §Rules.
//
// By default this adds its own toggle button as a Leaflet control (pass `button: false` to
// suppress it and drive `toggle()`/`active` from the page's own UI instead). A page with its
// own map click handler must check `active` and forward to `handleClick(latlng)` itself —
// Leaflet's multiple click listeners on one map can't otherwise be made to skip each other.
//
// options:
//   position   - Leaflet control position for the built-in button (default 'topleft')
//   button     - set false to suppress the built-in button
//   onToggle   - called with (isActive) whenever the tool is toggled on/off
//   onActivate - called right before the tool switches on (e.g. to cancel a conflicting mode)
function addMeasureTool(map, options = {}) {
    const state = { active: false, points: [], markers: [], line: null };
    let buttonEl = null;

    function haversineNm(lat1, lon1, lat2, lon2) {
        const R = 6371; // Earth radius in km
        const toRad = d => d * Math.PI / 180;
        const dLat = toRad(lat2 - lat1);
        const dLon = toRad(lon2 - lon1);
        const a = Math.sin(dLat / 2) ** 2 +
                  Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
        const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
        return (R * c) / 1.852;
    }

    function clearDrawing() {
        state.markers.forEach(m => map.removeLayer(m));
        state.markers = [];
        if (state.line) { map.removeLayer(state.line); state.line = null; }
        state.points = [];
    }

    function clear() {
        state.active = false;
        clearDrawing();
        map.getContainer().style.cursor = '';
        if (buttonEl) buttonEl.classList.remove('measure-tool-active');
        if (options.onToggle) options.onToggle(false);
    }

    function handleClick(latlng) {
        if (state.points.length >= 2) clearDrawing();

        state.points.push(latlng);
        state.markers.push(
            L.circleMarker(latlng, {
                color: '#fff', fillColor: '#f6c343', fillOpacity: 1, radius: 6, weight: 2
            }).addTo(map)
        );

        if (state.points.length === 2) {
            const [a, b] = state.points;
            const distNm = haversineNm(a.lat, a.lng, b.lat, b.lng);
            const text = distNm >= 1
                ? `${distNm.toFixed(2)} NM`
                : `${Math.round(distNm * 1852)} m`;
            state.line = L.polyline([[a.lat, a.lng], [b.lat, b.lng]], {
                color: '#f6c343', weight: 3, dashArray: '8,6'
            }).addTo(map);
            state.line.bindTooltip(text, {
                permanent: true, direction: 'center', className: 'measure-label'
            }).openTooltip();
        }
    }

    function toggle() {
        if (state.active) {
            clear();
            return;
        }
        if (options.onActivate) options.onActivate();
        state.active = true;
        map.getContainer().style.cursor = 'crosshair';
        if (buttonEl) buttonEl.classList.add('measure-tool-active');
        if (options.onToggle) options.onToggle(true);
    }

    if (options.button !== false) {
        const MeasureControl = L.Control.extend({
            options: { position: options.position || 'topleft' },
            onAdd() {
                const container = L.DomUtil.create('div', 'leaflet-bar measure-tool-control');
                buttonEl = L.DomUtil.create('a', '', container);
                buttonEl.href = '#';
                buttonEl.title = 'Measure distance';
                buttonEl.innerHTML = '📏';
                L.DomEvent.on(buttonEl, 'click', L.DomEvent.stop).on(buttonEl, 'click', toggle);
                return container;
            }
        });
        new MeasureControl().addTo(map);
    }

    return {
        toggle,
        clear,
        handleClick,
        get active() { return state.active; }
    };
}
