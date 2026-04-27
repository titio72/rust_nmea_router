# Trips Viewer — Deployment Guide

A read-only web viewer for trip data collected by the NMEA2000 router.
It runs the same binary (`nmea_router`) with CAN disabled, serving only the web UI and read-only REST API.

---

## How it differs from the full router

| Feature | Full router | Trips viewer |
|---------|-------------|--------------|
| CAN bus / NMEA2000 | Required | Disabled (`can.enabled: false`) |
| Write API (edit, delete, trim, import) | Available | Removed at startup |
| Sync push | `POST /api/sync/push` | Not available |
| Sync receive | Not available | `POST /api/sync/receive` |
| Authentication | Optional | Required (set `AUTH_PASSWORD`) |
| Intended host | On-board Linux | Cloud or local server |

Setting `can.enabled: false` in config automatically forces `read_only: true` — no write endpoints are registered regardless of the `web.read_only` value in the config file.

---

## Syncing trips from the boat

The boat runs `nmea_router` (full mode); the trips viewer is the sync target. The user triggers a sync manually, and the boat pushes all new or updated trips to the viewer. After sync, both databases contain the same trips.

### How it works

1. The boat calls `POST /api/sync/push`.
2. It sends all trip UUIDs + full data for trips changed since the last sync.
3. The viewer receives them at `POST /api/sync/receive`, deletes trips that no longer exist on the boat, and upserts the updated ones.
4. Both ends record the sync timestamp in `system_status`.

Data transfer is incremental: only trips whose `end_timestamp` is newer than the last sync are re-sent. On the first sync, all trips are transferred.

### Configuration

**On the boat** (`config.json`):

```json
"sync": {
  "enabled": true,
  "target_url": "https://your-trips-viewer.example.com",
  "api_key": "your-shared-secret-32-chars-min",
  "timeout_secs": 120
}
```

**On the trips viewer** (`/etc/trips_viewer/config.json` or Railway env vars):

```json
"sync": {
  "enabled": false,
  "target_url": "",
  "api_key": "your-shared-secret-32-chars-min",
  "timeout_secs": 120
}
```

The `api_key` must be identical on both ends. The viewer does not need `enabled: true` — it only receives, never pushes.

### Triggering a sync

From the boat, call the push endpoint (requires authentication):

```bash
curl -X POST http://boat-ip:8080/api/sync/push \
  -H "Cookie: session=<your-session-cookie>"
```

Or add a button in the web UI that calls `POST /api/sync/push`.

### Checking sync status

```bash
# On the boat or viewer
curl http://host:8080/api/sync/status
```

Response:
```json
{
  "status": "ok",
  "data": {
    "last_synced_at": "2026-04-25T10:00:00+00:00",
    "push_enabled": true
  }
}
```

### Security notes

- Use a long random string (≥ 32 chars) for `api_key`. It is sent as a Bearer token over HTTPS.
- The receive endpoint is always registered (even in read-only mode) but rejects requests without the correct key.
- The push endpoint is only registered on the boat (write mode). The trips viewer cannot initiate a sync.

---

## Option A — Local installation (Linux, systemd + nginx)

### Prerequisites

- Rust toolchain (`rustup`)
- MariaDB/MySQL with the `nmea_router` schema already loaded (`schema.sql`)
- nginx (`apt install nginx`)
- Root / sudo access

### 1. Build and install

Run from the repository root:

```bash
sudo ./deploy/install_trips_viewer.sh
```

This will:
- Build the `nmea_router` binary with `cargo build --release`
- Copy it to `/usr/local/bin/trips_viewer`
- Copy `static/` to `/opt/trips_viewer/static/`
- Create `/etc/trips_viewer/config.json` from `deploy/config.template.json` (only on first install)
- Install the systemd unit `/etc/systemd/system/trips_viewer.service`
- Install and enable the nginx reverse proxy config

### 2. Edit the config

```bash
sudo nano /etc/trips_viewer/config.json
```

Key fields to change:

```json
{
  "can": { "interface": "vcan0", "enabled": false },
  "database": {
    "connection": {
      "host": "localhost",
      "port": 3306,
      "username": "trips",
      "password": "CHANGE_ME",
      "database_name": "nmea_router"
    }
  },
  "web": {
    "enabled": true,
    "port": 8080,
    "read_only": true,
    "secure_cookies": true,
    "auth_password": "your-password-here"
  }
}
```

Set `secure_cookies: false` only if you are testing over plain HTTP without a TLS proxy.

### 3. Start the service

```bash
sudo systemctl enable trips_viewer.service
sudo systemctl start trips_viewer.service
sudo systemctl status trips_viewer.service
sudo journalctl -u trips_viewer.service -f   # stream logs
```

### 4. TLS certificate

The nginx config in `deploy/nginx.conf` uses a self-signed certificate placeholder.
For a real domain, replace it with Let's Encrypt:

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d yourdomain.com
```

Certbot will update `/etc/nginx/sites-available/trips_viewer` and reload nginx automatically.

### 5. Test locally without nginx (HTTP only)

Set `secure_cookies: false` in the config, then run directly:

```bash
./target/release/nmea_router
# or after install:
trips_viewer
```

Navigate to `http://localhost:8080`.

---

## Option B — Railway (cloud, Docker)

### Prerequisites

- A [Railway](https://railway.app) account
- The repository pushed to GitHub (Railway deploys from git)
- A database with the `nmea_router` schema — use Railway's MySQL plugin (see below)

### 1. Create a Railway project

1. Go to [railway.app](https://railway.app) → **New Project** → **Deploy from GitHub repo**
2. Select this repository
3. Railway will detect the `Dockerfile` and `railway.toml` automatically

### 2. Add the MySQL plugin

In the Railway project dashboard:
1. Click **+ New** → **Database** → **MySQL**
2. Railway creates the database and injects `DATABASE_URL` into your service automatically

Once the plugin is running, connect to it and load the schema:

```bash
# Get the connection details from the Railway MySQL plugin Variables tab
mysql -h <host> -P <port> -u <user> -p<password> <database> < schema.sql
```

### 3. Set environment variables

In the Railway service → **Variables** tab, add:

| Variable | Value | Notes |
|----------|-------|-------|
| `DATABASE_URL` | *(auto-set by MySQL plugin)* | `mysql://user:pass@host:port/dbname` |
| `AUTH_PASSWORD` | your chosen password | Min. 12 characters recommended |
| `SECURE_COOKIES` | `true` | Railway terminates TLS, so always true |
| `GOOGLE_MAPS_KEY` | your API key | Optional, enables map view |
| `LOG_LEVEL` | `info` | Optional (`trace`, `debug`, `info`, `warn`, `error`) |

`PORT` is injected by Railway automatically — do not set it manually.

### 4. Deploy

Push to the branch Railway is tracking (usually `master` or `main`). Railway builds the Docker image and deploys automatically.

Monitor the build log in the Railway dashboard. The healthcheck hits `/api/trips` — once it returns 200 the deployment is live.

### 5. Access the app

Railway provides a generated domain like `https://your-project.up.railway.app`.  
You can also add a custom domain in **Settings → Domains**.

---

## Environment variable reference

All variables override their counterparts in `config.json` at startup.

| Variable | Config field | Example |
|----------|-------------|---------|
| `DATABASE_URL` | `database.connection.*` | `mysql://user:pass@host:3306/db` |
| `PORT` | `web.port` | `8080` |
| `AUTH_PASSWORD` | `web.auth_password` | `mysecretpassword` |
| `SECURE_COOKIES` | `web.secure_cookies` | `true` |
| `GOOGLE_MAPS_KEY` | `web.google_maps_api_key` | `AIza...` |
| `LOG_LEVEL` | `logging.level` | `info` |

There is no environment variable override for `sync.api_key` — set it directly in `config.json` (or the Railway config file secret). Do not commit a real API key to source control.

---

## Deployment file reference

| File | Purpose |
|------|---------|
| `deploy/config.template.json` | Config template for local install (`can.enabled: false`) |
| `deploy/install_trips_viewer.sh` | Local install script (build + systemd + nginx) |
| `deploy/trips_viewer.service` | systemd unit template |
| `deploy/nginx.conf` | nginx reverse proxy config |
| `Dockerfile` | Multi-stage Docker build for Railway |
| `railway.toml` | Railway build/deploy settings |
