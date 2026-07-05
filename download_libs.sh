#!/bin/bash

# Downloads JavaScript frontend libraries into the specified directory.
# Usage: download_libs.sh <target_dir>
# Example: download_libs.sh /opt/nmea_router/static/libs

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

LIBS_DIR="${1:-static/libs}"

mkdir -p "$LIBS_DIR"

download() {
    local name="$1"
    local url="$2"
    curl -sSfL -o "$LIBS_DIR/$name" "$url" \
        && echo -e "${GREEN}✓ $name${NC}" \
        || echo -e "${YELLOW}⚠ Failed to download $name (offline?)${NC}"
}

echo -e "${YELLOW}Downloading JavaScript libraries into $LIBS_DIR...${NC}"

download "chart.min.js" \
    "https://cdn.jsdelivr.net/npm/chart.js@3.9.1/dist/chart.min.js"

download "date-fns.min.js" \
    "https://cdn.jsdelivr.net/npm/date-fns@2.29.3/index.min.js"

download "chartjs-adapter-date-fns.bundle.min.js" \
    "https://cdn.jsdelivr.net/npm/chartjs-adapter-date-fns@3.0.0/dist/chartjs-adapter-date-fns.bundle.min.js"

download "leaflet.min.js" \
    "https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js"

download "leaflet.min.css" \
    "https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.css"

download "leaflet.fullscreen.js" \
    "https://cdn.jsdelivr.net/npm/leaflet.fullscreen@3.0.2/Control.FullScreen.js"

download "leaflet.fullscreen.css" \
    "https://cdn.jsdelivr.net/npm/leaflet.fullscreen@3.0.2/Control.FullScreen.css"

download "icon-fullscreen.svg" \
    "https://cdn.jsdelivr.net/npm/leaflet.fullscreen@3.0.2/icon-fullscreen.svg"

# webgl-wind has no tagged releases, so it's pinned to a commit SHA.
download "wind-gl.js" \
    "https://cdn.jsdelivr.net/gh/mapbox/webgl-wind@b1f6468d90d2f39763a8795a5042f316a32ff3c8/dist/wind-gl.js"

# Patch wind-gl.js: its longitude-distortion correction assumes the wind texture
# covers the whole globe (pos.y*180-90), which is wrong once cropped to a single
# Forecast Area at real latitudes. Adds u_lat_min/u_lat_max uniforms so callers
# can supply the area's actual latitude range instead.
if [ -f "$LIBS_DIR/wind-gl.js" ]; then
    if ! grep -q "u_lat_min" "$LIBS_DIR/wind-gl.js"; then
        sed -i \
            -e 's/uniform float u_drop_rate_bump;\\n\\nvarying vec2 v_tex_pos;/uniform float u_drop_rate_bump;\\nuniform float u_lat_min;\\nuniform float u_lat_max;\\n\\nvarying vec2 v_tex_pos;/' \
            -e 's/float distortion = cos(radians(pos.y \* 180.0 - 90.0));/float distortion = cos(radians(mix(u_lat_max, u_lat_min, pos.y)));/' \
            -e 's/this.dropRateBump = 0.01; \/\/ drop rate increase relative to individual particle speed/&\n\n    this.latMin = -90;\n    this.latMax = 90;/' \
            -e 's/gl.uniform1f(program.u_drop_rate, this.dropRate);/&\n    gl.uniform1f(program.u_lat_min, this.latMin);\n    gl.uniform1f(program.u_lat_max, this.latMax);/' \
            "$LIBS_DIR/wind-gl.js"
        echo -e "${GREEN}✓ patched wind-gl.js (lat-aware distortion correction)${NC}"
    fi
fi
