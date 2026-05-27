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
