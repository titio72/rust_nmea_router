// Adds a small overlay panel to a Leaflet map showing the lat/lon under the mouse pointer.
// Visible only while hovering the map; hidden otherwise.
function addLatLonPanel(map, position = 'bottomleft') {
    const control = L.control({ position });
    control.onAdd = () => L.DomUtil.create('div', 'latLonPanel');
    control.addTo(map);

    const panelEl = control.getContainer();
    map.on('mousemove', (e) => {
        panelEl.style.display = 'block';
        panelEl.textContent = `Lat: ${e.latlng.lat.toFixed(5)}°  Lon: ${e.latlng.lng.toFixed(5)}°`;
    });
    map.on('mouseout', () => {
        panelEl.style.display = 'none';
    });

    return control;
}
