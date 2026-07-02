# Stage 1: builder
FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev perl make curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN mkdir -p static/libs && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/leaflet@1.9.4/dist/leaflet.min.css"                                          -o static/libs/leaflet.min.css && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/leaflet@1.9.4/dist/leaflet.min.js"                                           -o static/libs/leaflet.min.js && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/leaflet.fullscreen@5.3.3/dist/Control.FullScreen.css"                        -o static/libs/leaflet.fullscreen.css && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/leaflet.fullscreen@5.3.3/dist/Control.FullScreen.js"                         -o static/libs/leaflet.fullscreen.js && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/chart.js@3.9.1/dist/chart.min.js"                                            -o static/libs/chart.min.js && \
    curl -fsSL "https://cdn.jsdelivr.net/npm/chartjs-adapter-date-fns@3.0.0/dist/chartjs-adapter-date-fns.bundle.min.js"  -o static/libs/chartjs-adapter-date-fns.bundle.min.js

RUN cargo build --release --bin nmea_router

# Stage 2: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/trips_viewer

COPY --from=builder /app/target/release/nmea_router /usr/local/bin/nmea_router
COPY static/ ./static/
COPY --from=builder /app/static/libs/ ./static/libs/
COPY pgns.json .
COPY deploy_trips_viewer/config.template.json /etc/nmea_router/config.json

CMD ["nmea_router"]
