#!/bin/bash

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [ "$EUID" -ne 0 ]; then
    echo "Error: This script must be run as root (use sudo)"
    exit 1
fi

echo -e "${YELLOW}Stopping nmea_router service...${NC}"
systemctl stop nmea_router.service
echo -e "${GREEN}✓ Service stopped${NC}"

echo ""
bash "$(dirname "$0")/install.sh"

echo ""
echo -e "${YELLOW}Starting nmea_router service...${NC}"
systemctl start nmea_router.service
echo -e "${GREEN}✓ Service started${NC}"

echo ""
systemctl status nmea_router.service --no-pager
