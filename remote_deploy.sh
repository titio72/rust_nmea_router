#!/bin/bash

set -e

REMOTE_HOST="absail"
REMOTE_STAGE="nmea_router_deploy"
TARGET="armv7-unknown-linux-gnueabihf"
BINARY="target/${TARGET}/release/nmea_router"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Building for ${TARGET}...${NC}"
cross build --release --target "${TARGET}"
echo -e "${GREEN}✓ Build complete${NC}"

echo -e "${YELLOW}Syncing files to ${REMOTE_HOST}...${NC}"
rsync -av --delete \
    static/ \
    scripts/ \
    schema.sql \
    pgns.json \
    config.example.json \
    nmea_router.service \
    install.sh \
    deploy.sh \
    download_libs.sh \
    "${REMOTE_HOST}:${REMOTE_STAGE}/"

ssh "${REMOTE_HOST}" "mkdir -p ~/${REMOTE_STAGE}/target/release"
rsync -av "${BINARY}" "${REMOTE_HOST}:${REMOTE_STAGE}/target/release/nmea_router"
echo -e "${GREEN}✓ Files synced${NC}"

echo -e "${YELLOW}Deploying on ${REMOTE_HOST}...${NC}"
ssh "${REMOTE_HOST}" "cd ~/${REMOTE_STAGE} && sudo ./deploy.sh"
