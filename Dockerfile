# Stage 1: builder
FROM rust:1.86-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN cargo build --release --bin nmea_router

# Stage 2: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/trips_viewer

COPY --from=builder /app/target/release/nmea_router /usr/local/bin/nmea_router
COPY static/ ./static/
COPY pgns.json .
COPY deploy_trips_viewer/config.template.json /etc/nmea_router/config.json

CMD ["nmea_router"]
