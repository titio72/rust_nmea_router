#!/bin/bash

# NMEA Router Installation Script
# This script installs the nmea_router service system-wide

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
WORK_DIR="/opt/nmea_router"
CONFIG_DIR="/etc/nmea_router"
BINARY_PATH="/usr/local/bin/nmea_router"
LOG_DIR="/var/log/nmea_router"
SERVICE_FILE="nmea_router.service"
CONFIG_FILE="config.json"
SCHEMA_FILE="schema.sql"
PGN_FILE="pgns.json"

echo -e "${GREEN}NMEA Router Installation Script${NC}"
echo "=================================="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Error: This script must be run as root (use sudo)${NC}"
    exit 1
fi

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo not found. Please install Rust first.${NC}"
    exit 1
fi

# Get the current user (the one who invoked sudo)
ACTUAL_USER=${SUDO_USER:-$USER}
ACTUAL_GROUP=$(id -gn $ACTUAL_USER)

echo -e "${YELLOW}Building release binary...${NC}"
sudo -u $ACTUAL_USER cargo build --release
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Create working directory
echo -e "${YELLOW}Creating working directory...${NC}"
mkdir -p "$WORK_DIR"
echo -e "${GREEN}✓ Created $WORK_DIR${NC}"

# Create configuration directory
echo -e "${YELLOW}Creating configuration directory...${NC}"
mkdir -p "$CONFIG_DIR"
echo -e "${GREEN}✓ Created $CONFIG_DIR${NC}"

# Copy binary
echo -e "${YELLOW}Installing binary...${NC}"
cp target/release/nmea_router "$BINARY_PATH"
chmod +x "$BINARY_PATH"
echo -e "${GREEN}✓ Installed to $BINARY_PATH${NC}"

# Copy configuration files
echo -e "${YELLOW}Installing configuration files...${NC}"
cp "$CONFIG_FILE" "$CONFIG_DIR/"
[ -f "$SCHEMA_FILE" ] && cp "$SCHEMA_FILE" "$CONFIG_DIR/"
[ -f "$PGN_FILE" ] && cp "$PGN_FILE" "$CONFIG_DIR/"
[ -f "config.example.json" ] && cp "config.example.json" "$CONFIG_DIR/"
echo -e "${GREEN}✓ Configuration files copied${NC}"

# Update config.json to use /var/log/nmea_router
echo -e "${YELLOW}Updating log directory in config...${NC}"
if [ -f "$CONFIG_DIR/$CONFIG_FILE" ]; then
    # Use sed to update the log directory in the config file
    sed -i 's|"directory": "logs"|"directory": "'$LOG_DIR'"|g' "$CONFIG_DIR/$CONFIG_FILE"
    echo -e "${GREEN}✓ Log directory updated to $LOG_DIR${NC}"
fi

# Create log directory
echo -e "${YELLOW}Creating log directory...${NC}"
mkdir -p "$LOG_DIR"
chown $ACTUAL_USER:$ACTUAL_GROUP "$LOG_DIR"
chmod 755 "$LOG_DIR"
echo -e "${GREEN}✓ Created $LOG_DIR${NC}"

# Set ownership of working directory
chown -R $ACTUAL_USER:$ACTUAL_GROUP "$WORK_DIR"
chmod 755 "$WORK_DIR"

# Set ownership of configuration directory
chown -R root:root "$CONFIG_DIR"
chmod 755 "$CONFIG_DIR"
chmod 644 "$CONFIG_DIR"/*.json 2>/dev/null || true
echo ""

# Update service file with actual user
echo -e "${YELLOW}Installing systemd service...${NC}"
cp "$SERVICE_FILE" /etc/systemd/system/
# Update the user in the service file
sed -i "s/User=aboni/User=$ACTUAL_USER/g" /etc/systemd/system/"$SERVICE_FILE"
sed -i "s/Group=aboni/Group=$ACTUAL_GROUP/g" /etc/systemd/system/"$SERVICE_FILE"
echo -e "${GREEN}✓ Service file installed${NC}"

# Reload systemd
echo -e "${YELLOW}Reloading systemd...${NC}"
systemctl daemon-reload
echo -e "${GREEN}✓ Systemd reloaded${NC}"
echo ""

echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo "Directory structure:"
echo "  Binary:        $BINARY_PATH"
echo "  Working Dir:   $WORK_DIR"
echo "  Config:        $CONFIG_DIR"
echo "  Logs:          $LOG_DIR"
echo "  Service:       /etc/systemd/system/$SERVICE_FILE"
echo ""
echo "Next steps:"
echo "  1. Review and edit configuration: sudo nano $CONFIG_DIR/$CONFIG_FILE"
echo "  2. Enable service: sudo systemctl enable $SERVICE_FILE"
echo "  3. Start service:  sudo systemctl start $SERVICE_FILE"
echo "  4. Check status:   sudo systemctl status $SERVICE_FILE"
echo "  5. View logs:      sudo journalctl -u $SERVICE_FILE -f"
echo ""
echo -e "${YELLOW}Note: Make sure to configure your CAN interface and database settings before starting.${NC}"
