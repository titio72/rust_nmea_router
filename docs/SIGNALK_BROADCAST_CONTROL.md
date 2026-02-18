# SignalK Broadcast Control Implementation

## Overview
This document describes the implementation of dynamic SignalK broadcast enabling/disabling functionality, allowing users to control SignalK broadcasting at runtime through the web UI while persisting the preference in the database.

## Changes Made

### 1. API Endpoints (src/web/api.rs)

Added two new API endpoints to manage SignalK broadcast status:

#### Structures
- `SignalKStatusRequest` - Request structure containing `enabled: bool`
- `SignalKStatusResponse` - Response structure containing `enabled: bool`

#### Endpoints
- `GET /api/signalk/status` - Retrieves the current SignalK broadcast status from the database
- `POST /api/signalk/status` - Updates the SignalK broadcast status and persists it to the database

The endpoints follow the same pattern as the existing tracking and metrics status endpoints.

### 2. Web UI (static/index.html)

Added a new toggle control in the settings section:

#### HTML
- Added a new toggle container with id `signalkToggle` next to the existing tracking and metrics toggles
- Uses the same styling classes as other toggles for consistency

#### JavaScript Functions
- `initializeSignalk()` - Loads the current SignalK broadcast status from the database on page load
- `toggleSignalk()` - Handles user clicks on the SignalK toggle, updating the status in the database

Both functions follow the same pattern as the existing `initializeTracking()`, `toggleTracking()`, `initializeMetrics()`, and `toggleMetrics()` functions.

### 3. Main Application Logic (src/main.rs)

#### New Function
Added `is_signalk_enabled()` function that:
- Checks the database for the `signalk_enabled` system status flag
- Returns false if no database connection is available
- Returns false if the flag is not set

#### Modified Logic
Updated the SignalK broadcaster message handling to check both:
- Whether the broadcaster exists (based on config.signalk.enabled)
- Whether SignalK is enabled in the database via `is_signalk_enabled()`

This allows dynamic runtime control without needing to restart the application.

## How It Works

### Startup
1. Application starts and loads configuration (including the initial config.signalk.enabled setting)
2. SignalK broadcaster is created if `config.signalk.enabled` is true
3. When processing NMEA2000 messages, the application checks both the broadcaster existence AND the database flag before sending SignalK data

### User Interaction
1. User toggles the SignalK broadcast switch in the web UI
2. JavaScript sends a POST request to `/api/signalk/status` with the new status
3. API updates the `signalk_enabled` flag in the database `system_status` table
4. Subsequent NMEA2000 messages are processed accordingly - SignalK messages are sent or not based on the database flag

### Database Persistence
- The state is stored in the `system_status` table with key `signalk_enabled`
- The state persists across application restarts
- The state survives application crashes

## Database
Uses existing `system_status` table:
```sql
INSERT INTO system_status (key, value) VALUES ('signalk_enabled', true/false)
```

The database operations use the same `get_system_status()` and `set_system_status()` methods already defined in the VesselDatabase class.

## API Examples

### Get Status
```bash
curl http://localhost:8000/api/signalk/status
```

Response (enabled):
```json
{
  "status": "ok",
  "data": {
    "enabled": true
  },
  "error": null
}
```

### Set Status
```bash
curl -X POST http://localhost:8000/api/signalk/status \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

## Default Behavior
- If SignalK broadcasting is disabled in the configuration (`config.signalk.enabled: false`), no broadcaster is created
- If the database is unavailable, SignalK broadcasting defaults to disabled
- If no entry exists in the database for `signalk_enabled`, it defaults to false
- Configuration changes require application restart; database changes take effect immediately

## Consistency with Existing Patterns
This implementation follows the exact same patterns as:
- Vessel tracking enable/disable (`tracking_enabled`)
- Metrics collection enable/disable (`metrics_enabled`)

Both of these features use identical API endpoints, UI toggles, and database persistence mechanisms.
