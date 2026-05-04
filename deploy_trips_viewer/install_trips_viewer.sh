#!/usr/bin/env bash
set -euo pipefail

# Install trips viewer as a systemd service with nginx reverse proxy.
# Run from the repository root: sudo ./deploy_trips_viewer/install_trips_viewer.sh

if [ "$EUID" -ne 0 ]; then
    echo "Error: run as root (sudo $0)"
    exit 1
fi

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEPLOY_USER="${SUDO_USER:-$(id -un)}"
BINARY_SRC="$REPO_DIR/target/release/nmea_router"
INSTALL_DIR="/opt/trips_viewer"
CONFIG_DIR="/etc/trips_viewer"
LOG_DIR="/var/log/trips_viewer"
BINARY_DST="/usr/local/bin/trips_viewer"
SERVICE_FILE="/etc/systemd/system/trips_viewer.service"
NGINX_AVAIL="/etc/nginx/sites-available/trips_viewer"
NGINX_ENABLED="/etc/nginx/sites-enabled/trips_viewer"

# Build
echo "==> Building..."
CARGO="$(eval echo ~"$DEPLOY_USER")/.cargo/bin/cargo"
sudo -u "$DEPLOY_USER" "$CARGO" build --release --bin nmea_router --manifest-path "$REPO_DIR/Cargo.toml"

# Directories
echo "==> Creating directories..."
mkdir -p "$INSTALL_DIR/static" "$CONFIG_DIR" "$LOG_DIR"
chown "$DEPLOY_USER:$DEPLOY_USER" "$INSTALL_DIR" "$LOG_DIR"
chmod 755 "$CONFIG_DIR"

# Binary (copy with distinct name to avoid conflict with nmea_router service)
echo "==> Installing binary..."
cp "$BINARY_SRC" "$BINARY_DST"
chmod 755 "$BINARY_DST"

# Static files
echo "==> Installing static files..."
cp -r "$REPO_DIR/static/." "$INSTALL_DIR/static/"
chown -R "$DEPLOY_USER:$DEPLOY_USER" "$INSTALL_DIR/static"

# Config
if [ ! -f "$CONFIG_DIR/config.json" ]; then
    echo "==> Installing default config (edit before starting service)..."
    cp "$REPO_DIR/deploy_trips_viewer/config.template.json" "$CONFIG_DIR/config.json"
    sed -i "s|/var/log/trips_viewer|$LOG_DIR|g" "$CONFIG_DIR/config.json"
    chmod 644 "$CONFIG_DIR/config.json"
else
    echo "==> Config already exists at $CONFIG_DIR/config.json — skipping"
fi

# Systemd service
echo "==> Installing systemd service..."
sed "s/__USER__/$DEPLOY_USER/g" "$REPO_DIR/deploy_trips_viewer/trips_viewer.service" > "$SERVICE_FILE"
systemctl daemon-reload

# Nginx
if command -v nginx &>/dev/null; then
    echo "==> Installing nginx config..."
    cp "$REPO_DIR/deploy_trips_viewer/nginx.conf" "$NGINX_AVAIL"
    if [ ! -L "$NGINX_ENABLED" ]; then
        ln -s "$NGINX_AVAIL" "$NGINX_ENABLED"
    fi
    nginx -t && systemctl reload nginx || echo "WARNING: nginx config test failed — check $NGINX_AVAIL"
else
    echo "==> nginx not found, skipping reverse proxy setup"
fi

echo ""
echo "==> Installation complete!"
echo ""
echo "Next steps:"
echo "  1. Edit config:  sudo nano $CONFIG_DIR/config.json"
echo "  2. Enable:       sudo systemctl enable trips_viewer.service"
echo "  3. Start:        sudo systemctl start trips_viewer.service"
echo "  4. Logs:         sudo journalctl -u trips_viewer.service -f"
echo "  5. TLS cert:     sudo certbot --nginx -d yourdomain.com"
