// GPU wind particle animation for the planning map (static/plan.html).
//
// One WindParticleLayer per Forecast Area: a canvas + WebGL context + vendored
// WindGL instance (static/libs/wind-gl.js), clipped to that area's on-screen
// rectangle. The field is rebuilt from data already fetched by plan.html
// (lastGridPts via /api/forecast/grid-points) — no network calls of its own.
//
// The wind field is rasterized ONCE per data load (or gust/wind toggle) over the area's
// full bounds and uploaded to the GPU. Pan/zoom only resizes the canvas, tells the
// shader which sub-rectangle of that texture the canvas shows (texOrigin/texScale), and
// repaints the background wash from the cached raster — no IDW work per gesture.
//
// Depends on globals defined in plan.html: buildSpatialIndex, idwInterpolateIndexed,
// nativeGridSteps, lastGridPts, showGust, windColor.

const KN_TO_MS = 0.514444;

// Particle streaks are plain white — wind intensity is conveyed by the separate
// static-colored background layer (bgCanvas) instead, using windColor() from plan.html.
const WHITE_RAMP = { '0.0': '#ffffff', '1.0': '#ffffff' };

// Alpha (0-255) of the translucent intensity wash, so map tiles stay visible underneath.
const INTENSITY_ALPHA = 140;

// windColor() returns an "rgb(r,g,b)" string; parsing it per texel is expensive, so cache
// the parsed channels per 0.1 kn bucket (the scale is smooth, so the quantization is invisible).
const rgbCache = new Map();
function windRgb(speedKn) {
    const key = Math.round(speedKn * 10);
    let rgb = rgbCache.get(key);
    if (!rgb) {
        rgb = windColor(key / 10).match(/\d+/g).map(Number);
        rgbCache.set(key, rgb);
    }
    return rgb;
}

function pointsInArea(pts, area) {
    return pts.filter(p =>
        p.lat >= area.lat_min && p.lat <= area.lat_max &&
        p.lon >= area.lon_min && p.lon <= area.lon_max);
}

// Intersection of a forecast area's bounds with the current map viewport, both
// as a geographic box and the on-screen pixel rectangle it projects to.
// Returns null if the area is entirely off-screen.
function computeVisibleRect(map, area) {
    const mapBounds = map.getBounds();
    const latMin = Math.max(area.lat_min, mapBounds.getSouth());
    const latMax = Math.min(area.lat_max, mapBounds.getNorth());
    const lonMin = Math.max(area.lon_min, mapBounds.getWest());
    const lonMax = Math.min(area.lon_max, mapBounds.getEast());
    if (latMin >= latMax || lonMin >= lonMax) return null;

    const topLeft = map.latLngToContainerPoint([latMax, lonMin]);
    const bottomRight = map.latLngToContainerPoint([latMin, lonMax]);
    const left = Math.round(Math.min(topLeft.x, bottomRight.x));
    const top = Math.round(Math.min(topLeft.y, bottomRight.y));
    const width = Math.round(Math.abs(bottomRight.x - topLeft.x));
    const height = Math.round(Math.abs(bottomRight.y - topLeft.y));
    if (width < 2 || height < 2) return null;

    return { left, top, width, height, latMin, latMax, lonMin, lonMax };
}

// Rasterize IDW-interpolated wind onto a texW x texH grid covering `bounds`, producing
// both the u/v vector texture WindGL needs and an RGBA intensity image (color-by-speed,
// via the same windColor() scale used elsewhere on the page) for the static background layer.
// Row 0 = bounds.latMax (north), row texH-1 = bounds.latMin (south) — see the
// u_lat_min/u_lat_max patch in wind-gl.js, which assumes this orientation.
function rasterizeWindField(pts, texW, texH, bounds, useGust) {
    if (!pts.length) return null;
    const index = buildSpatialIndex(pts);

    const u = new Float64Array(texW * texH);
    const v = new Float64Array(texW * texH);
    const speedKn = new Float64Array(texW * texH);
    const valid = new Uint8Array(texW * texH);
    let uMin = Infinity, uMax = -Infinity, vMin = Infinity, vMax = -Infinity;
    let any = false;

    for (let row = 0; row < texH; row++) {
        const latT = texH > 1 ? row / (texH - 1) : 0;
        const lat = bounds.latMax - latT * (bounds.latMax - bounds.latMin);
        for (let col = 0; col < texW; col++) {
            const lonT = texW > 1 ? col / (texW - 1) : 0;
            const lon = bounds.lonMin + lonT * (bounds.lonMax - bounds.lonMin);
            const pt = idwInterpolateIndexed(lat, lon, index);

            const idx = row * texW + col;
            let uu = 0, vv = 0;
            if (pt && pt.wind_direction_deg != null) {
                const spdKn = useGust ? (pt.wind_gust_kn ?? pt.wind_speed_kn ?? 0)
                                       : (pt.wind_speed_kn ?? 0);
                const speedMs = spdKn * KN_TO_MS;
                // wind_direction_deg is where the wind comes FROM; rotate +180 for "blowing towards".
                const towardRad = ((pt.wind_direction_deg + 180) % 360) * Math.PI / 180;
                uu = speedMs * Math.sin(towardRad);
                vv = speedMs * Math.cos(towardRad);
                speedKn[idx] = spdKn;
                valid[idx] = 1;
                any = true;
            }

            u[idx] = uu;
            v[idx] = vv;
            if (uu < uMin) uMin = uu;
            if (uu > uMax) uMax = uu;
            if (vv < vMin) vMin = vv;
            if (vv > vMax) vMax = vv;
        }
    }
    if (!any) return null;
    // Avoid a zero-width range (e.g. dead calm everywhere), which would divide by zero in the shader.
    if (uMax === uMin) uMax = uMin + 0.01;
    if (vMax === vMin) vMax = vMin + 0.01;

    const vectorBytes = new Uint8ClampedArray(texW * texH * 4);
    const intensityBytes = new Uint8ClampedArray(texW * texH * 4);
    for (let i = 0; i < texW * texH; i++) {
        vectorBytes[i * 4]     = 255 * (u[i] - uMin) / (uMax - uMin);
        vectorBytes[i * 4 + 1] = 255 * (v[i] - vMin) / (vMax - vMin);
        vectorBytes[i * 4 + 2] = 0;
        vectorBytes[i * 4 + 3] = 255;

        if (valid[i]) {
            const [r, g, b] = windRgb(speedKn[i]);
            intensityBytes[i * 4] = r;
            intensityBytes[i * 4 + 1] = g;
            intensityBytes[i * 4 + 2] = b;
            intensityBytes[i * 4 + 3] = INTENSITY_ALPHA;
        }
    }

    return {
        vector: { image: new ImageData(vectorBytes, texW, texH), width: texW, height: texH, uMin, uMax, vMin, vMax },
        intensity: new ImageData(intensityBytes, texW, texH),
    };
}

const activeWindLayers = new Set();
(function tick() {
    activeWindLayers.forEach(layer => layer.draw());
    requestAnimationFrame(tick);
})();

// Wind texture density: ~2 texels per native forecast-grid step (GPU bilinear filtering
// smooths between them), clamped so one full-area IDW pass stays cheap.
const TEX_PER_GRID_STEP = 2;
const TEX_MIN_SIDE = 8;
const TEX_MAX_SIDE = 256;

class WindParticleLayer {
    constructor(area) {
        this.area = area;
        this.map = null;
        this.bgCanvas = null;   // static color-by-speed intensity wash
        this.canvas = null;     // WebGL particle streaks (white), drawn on top
        this.bgCtx = null;
        this.gl = null;
        this.wind = null;
        this.visible = false;
        this.visibleBounds = null;
        this.rebuildTimer = null;
        this.enabled = true;   // false when the user has toggled the wind overlay off
        this.fieldDirty = true;        // data or gust/wind mode changed since the last rasterization
        this.intensityCanvas = null;   // cached full-area color-by-speed raster (texW x texH)
        // Canvas sub-rectangle of the full-area texture, as fractions (see reposition()).
        this.texOrigin = [0, 0];
        this.texScale = [1, 1];
    }

    onAdd(map) {
        this.map = map;
        const pane = map.getPanes().overlayPane;

        this.bgCanvas = document.createElement('canvas');
        this.bgCanvas.style.position = 'absolute';
        this.bgCanvas.style.pointerEvents = 'none';
        pane.appendChild(this.bgCanvas);
        this.bgCtx = this.bgCanvas.getContext('2d');

        this.canvas = document.createElement('canvas');
        this.canvas.style.position = 'absolute';
        this.canvas.style.pointerEvents = 'none';
        pane.appendChild(this.canvas);

        const gl = this.canvas.getContext('webgl', { antialias: false });
        if (!gl) {
            console.warn('WebGL unavailable; wind particles disabled for forecast area', this.area.id);
            return;
        }
        this.gl = gl;
        this.wind = new WindGL(gl);
        this.wind.setColorRamp(WHITE_RAMP);
        activeWindLayers.add(this);
        this.reposition();
    }

    onRemove() {
        activeWindLayers.delete(this);
        if (this.rebuildTimer) { clearTimeout(this.rebuildTimer); this.rebuildTimer = null; }
        if (this.canvas) this.canvas.remove();
        if (this.bgCanvas) this.bgCanvas.remove();
    }

    // shared.css forces `canvas { width:100% !important; height:auto !important }` for
    // responsive charts elsewhere on the page; override it here so canvases keep their
    // actual pixel size instead of collapsing to the overlayPane's intrinsic 0x0.
    static sizeCanvas(canvas, width, height) {
        canvas.width = width;
        canvas.height = height;
        canvas.style.setProperty('width', width + 'px', 'important');
        canvas.style.setProperty('height', height + 'px', 'important');
    }

    // Shows/hides the canvases without tearing down the WebGL context, so re-enabling
    // doesn't need a network refetch or texture rebuild. Called from plan.html's wind
    // overlay checkbox.
    setEnabled(enabled) {
        this.enabled = enabled;
        this.reposition();
    }

    // Called on moveend/zoomend and on initial add.
    reposition() {
        if (!this.gl) return;
        if (this.rebuildTimer) { clearTimeout(this.rebuildTimer); this.rebuildTimer = null; }
        if (!this.enabled) {
            this.visible = false;
            this.visibleBounds = null;
            this.canvas.style.display = 'none';
            this.bgCanvas.style.display = 'none';
            return;
        }
        const rect = computeVisibleRect(this.map, this.area);
        if (!rect) {
            this.visible = false;
            this.visibleBounds = null;
            this.canvas.style.display = 'none';
            this.bgCanvas.style.display = 'none';
            return;
        }
        this.visible = true;
        this.visibleBounds = rect;
        this.canvas.style.display = '';
        this.bgCanvas.style.display = '';
        WindParticleLayer.sizeCanvas(this.canvas, rect.width, rect.height);
        WindParticleLayer.sizeCanvas(this.bgCanvas, rect.width, rect.height);
        const layerPoint = this.map.latLngToLayerPoint([rect.latMax, rect.lonMin]);
        L.DomUtil.setPosition(this.canvas, layerPoint);
        L.DomUtil.setPosition(this.bgCanvas, layerPoint);

        this.wind.resize();
        this.wind.latMin = rect.latMin;
        this.wind.latMax = rect.latMax;
        this.wind.numParticles = Math.min(4096, Math.max(256, Math.round((rect.width * rect.height) / 200)));

        // The wind texture spans the whole area (row 0 = north edge); tell the shaders which
        // slice of it this canvas shows. Same linear-in-lat/lon mapping the old per-viewport
        // texture used, just offset/scaled.
        const a = this.area;
        const lonSpan = a.lon_max - a.lon_min;
        const latSpan = a.lat_max - a.lat_min;
        this.texOrigin = [(rect.lonMin - a.lon_min) / lonSpan, (a.lat_max - rect.latMax) / latSpan];
        this.texScale = [(rect.lonMax - rect.lonMin) / lonSpan, (rect.latMax - rect.latMin) / latSpan];
        this.wind.texOrigin = this.texOrigin;
        this.wind.texScale = this.texScale;

        if (!this.fieldDirty) {
            // Cheap path (every pan/zoom): the field is already on the GPU, only the
            // background wash needs to be re-sliced from the cached raster.
            this.paintBackground();
            return;
        }

        // The IDW rasterization is the expensive part. Run it in its own task so the canvas
        // resize/reposition above can paint first instead of freezing the overlay.
        this.rebuildTimer = setTimeout(() => {
            this.rebuildTimer = null;
            this.buildField();
        }, 0);
    }

    // Called when lastGridPts or showGust changes: invalidates the cached field. It is
    // rasterized right away if the area is on screen, otherwise on the next reposition()
    // that brings it into view.
    rebuildTexture() {
        if (!this.gl) return;
        this.fieldDirty = true;
        if (!this.visible || !this.visibleBounds) return;
        if (this.rebuildTimer) { clearTimeout(this.rebuildTimer); this.rebuildTimer = null; }
        this.buildField();
    }

    // Rasterizes the wind field over the area's FULL bounds (independent of the viewport),
    // uploads the vector texture, and caches the color-by-speed raster for the background.
    buildField() {
        if (!this.gl) return;
        const a = this.area;
        const bounds = { latMin: a.lat_min, latMax: a.lat_max, lonMin: a.lon_min, lonMax: a.lon_max };
        const pts = pointsInArea(lastGridPts, a);

        let texW = TEX_MAX_SIDE, texH = TEX_MAX_SIDE;
        if (pts.length >= 2) {
            const step = nativeGridSteps(pts);
            const clamp = n => Math.min(TEX_MAX_SIDE, Math.max(TEX_MIN_SIDE, Math.ceil(n)));
            texW = clamp((bounds.lonMax - bounds.lonMin) / step.lon * TEX_PER_GRID_STEP);
            texH = clamp((bounds.latMax - bounds.latMin) / step.lat * TEX_PER_GRID_STEP);
        }

        const field = rasterizeWindField(pts, texW, texH, bounds, showGust);
        this.fieldDirty = false;
        if (!field) {
            this.wind.windData = null;
            this.intensityCanvas = null;
            this.bgCtx.clearRect(0, 0, this.bgCanvas.width, this.bgCanvas.height);
            return;
        }
        this.wind.setWind(field.vector);

        const off = document.createElement('canvas');
        off.width = texW;
        off.height = texH;
        off.getContext('2d').putImageData(field.intensity, 0, 0);
        this.intensityCanvas = off;
        this.paintBackground();
    }

    // Paints the visible slice of the cached intensity raster into the background canvas;
    // image smoothing turns the coarse grid into a soft gradient wash.
    paintBackground() {
        const ctx = this.bgCtx;
        ctx.clearRect(0, 0, this.bgCanvas.width, this.bgCanvas.height);
        const src = this.intensityCanvas;
        if (!src || !this.visible) return;
        const sx = this.texOrigin[0] * src.width;
        const sy = this.texOrigin[1] * src.height;
        const sw = Math.min(this.texScale[0] * src.width, src.width - sx);
        const sh = Math.min(this.texScale[1] * src.height, src.height - sy);
        if (sw <= 0 || sh <= 0) return;
        ctx.imageSmoothingEnabled = true;
        ctx.drawImage(src, sx, sy, sw, sh, 0, 0, this.bgCanvas.width, this.bgCanvas.height);
    }

    draw() {
        if (this.visible && this.wind && this.wind.windData) this.wind.draw();
    }
}
