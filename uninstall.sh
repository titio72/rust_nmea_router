#!/bin/bash

# NMEA Router Uninstallation Script

set -e

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

echo -e "${YELLOW}NMEA Router Uninstallation Script${NC}"
echo "===================================="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Error: This script must be run as root (use sudo)${NC}"
    exit 1
fi

# Stop the service if running
if systemctl is-active --quiet $SERVICE_FILE; then
    echo -e "${YELLOW}Stopping service...${NC}"
    systemctl stop $SERVICE_FILE
    echo -e "${GREEN}✓ Service stopped${NC}"
fi

# Disable the service if enabled
if systemctl is-enabled --quiet $SERVICE_FILE 2>/dev/null; then
    echo -e "${YELLOW}Disabling service...${NC}"
    systemctl disable $SERVICE_FILE
    echo -e "${GREEN}✓ Service disabled${NC}"
fi

# Remove service file
if [ -f "/etc/systemd/system/$SERVICE_FILE" ]; then
    echo -e "${YELLOW}Removing service file...${NC}"
    rm /etc/systemd/system/$SERVICE_FILE
    systemctl daemon-reload
    echo -e "${GREEN}✓ Service file removed${NC}"
fi

# Remove binary
if [ -f "$BINARY_PATH" ]; then
    echo -e "${YELLOW}Removing binary...${NC}"
    rm "$BINARY_PATH"
    echo -e "${GREEN}✓ Binary removed${NC}"
fi

# Remove working directory
if [ -d "$WORK_DIR" ]; then
    echo -e "${YELLOW}Removing working directory...${NC}"
    read -p "Remove $WORK_DIR? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$WORK_DIR"
        echo -e "${GREEN}✓ Working directory removed${NC}"
    else
        echo -e "${YELLOW}Skipped removing $WORK_DIR${NC}"
    fi
fi

# Remove configuration directory
if [ -d "$CONFIG_DIR" ]; then
    echo -e "${YELLOW}Removing configuration directory...${NC}"
    read -p "Remove $CONFIG_DIR? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$CONFIG_DIR"
        echo -e "${GREEN}✓ Configuration directory removed${NC}"
    else
        echo -e "${YELLOW}Skipped removing $CONFIG_DIR${NC}"
    fi
fi

# Ask about log directory
if [ -d "$LOG_DIR" ]; then
    echo -e "${YELLOW}Log directory found: $LOG_DIR${NC}"
    read -p "Remove log directory and all logs? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$LOG_DIR"
        echo -e "${GREEN}✓ Log directory removed${NC}"
    else
        echo -e "${YELLOW}Preserved log directory: $LOG_DIR${NC}"
    fi
fi

echo ""
echo -e "${GREEN}Uninstallation complete!${NC}"
