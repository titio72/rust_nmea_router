# NMEA Router REST API Documentation

## Overview

The NMEA Router provides a RESTful API for accessing vessel tracking data, trip information, and environmental metrics. The API is built with Axum and provides JSON responses for all endpoints.

**Base URL**: `http://localhost:8080/api`

**Default Port**: 8080 (configurable in `config.json`)

## Response Format

All API responses follow a consistent format:

```json
{
  "status": "ok" | "error",
  "data": <response_data> | null,
  "error": "<error_message>" | null
}
```

### Success Response
```json
{
  "status": "ok",
  "data": { ... },
  "error": null
}
```

### Error Response
```json
{
  "status": "error",
  "data": null,
  "error": "Error description"
}
```

---

## Endpoints

### 1. Get Trips

Retrieve a list of trips with optional filtering by year or recent months.

**Endpoint**: `GET /api/trips`

**Query Parameters**:
- `year` (optional, integer): Filter trips by specific year (e.g., `2026`)
- `last_months` (optional, integer): Filter trips from the last N months

**Examples**:
```bash
# Get all trips
curl http://localhost:8080/api/trips

# Get trips from 2026
curl http://localhost:8080/api/trips?year=2026

# Get trips from the last 6 months
curl http://localhost:8080/api/trips?last_months=6
```

**Response**:
```json
{
  "status": "ok",
  "data": [
    {
      "id": 132,
      "description": "Trip to Marina Bay",
      "start_date": "2026-02-02T08:30:00",
      "end_date": "2026-02-02T16:45:00",
      "total_distance_nm": 45.3,
      "total_time_ms": 29700000,
      "sailing_time_ms": 18000000,
      "motoring_time_ms": 7200000,
      "moored_time_ms": 4500000,
      "sailing_distance_nm": 32.1,
      "motoring_distance_nm": 13.2
    }
  ],
  "error": null
}
```

**Fields**:
- `id`: Unique trip identifier
- `description`: User-provided trip description
- `start_date`: Trip start timestamp (ISO 8601 format)
- `end_date`: Trip end timestamp (ISO 8601 format)
- `total_distance_nm`: Total distance traveled in nautical miles
- `total_time_ms`: Total trip duration in milliseconds
- `sailing_time_ms`: Time spent sailing in milliseconds
- `motoring_time_ms`: Time spent motoring in milliseconds
- `moored_time_ms`: Time spent moored in milliseconds
- `sailing_distance_nm`: Distance covered while sailing in nautical miles
- `motoring_distance_nm`: Distance covered while motoring in nautical miles

---

### 2. Get Trip Details

Retrieve detailed information about a specific trip.

**Endpoint**: `GET /api/trip`

**Query Parameters**:
- `id` (required, integer): Trip ID

**Example**:
```bash
curl http://localhost:8080/api/trip?id=132
```

**Response**:
```json
{
  "status": "ok",
  "data": {
    "id": 132,
    "description": "Trip to Marina Bay",
    "start_date": "2026-02-02T08:30:00",
    "end_date": "2026-02-02T16:45:00",
    "total_distance_nm": 45.3,
    "total_time_ms": 29700000,
    "sailing_time_ms": 18000000,
    "motoring_time_ms": 7200000,
    "moored_time_ms": 4500000,
    "sailing_distance_nm": 32.1,
    "motoring_distance_nm": 13.2
  },
  "error": null
}
```

**Error Response** (Trip not found):
```json
{
  "status": "error",
  "data": null,
  "error": "Trip 999 not found"
}
```

---

### 3. Get Track Data

Retrieve GPS track points for visualization on a map. Can be filtered by trip ID or date range.

**Endpoint**: `GET /api/track`

**Query Parameters**:
- `trip_id` (optional, integer): Filter by specific trip
- `start` (optional, string): Start date/time (ISO 8601 format)
- `end` (optional, string): End date/time (ISO 8601 format)

**Examples**:
```bash
# Get track for a specific trip
curl http://localhost:8080/api/track?trip_id=132

# Get track for a date range
curl "http://localhost:8080/api/track?start=2026-02-01T00:00:00&end=2026-02-03T23:59:59"

# Get all track data
curl http://localhost:8080/api/track
```

**Response**:
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2026-02-02T08:30:15",
      "latitude": 45.5231,
      "longitude": -122.6765,
      "avg_speed_kn": 5.2,
      "max_speed_kn": 6.8,
      "moored": false,
      "engine_on": false,
      "total_distance_nm": 2.3,
      "total_time_ms": 1800000,
      "average_wind_speed_kn": 12.5,
      "average_wind_angle_deg": 45.0,
      "cog_deg": 135.0,
      "average_heading_deg": 138.0
    },
    {
      "timestamp": "2026-02-02T08:40:15",
      "latitude": 45.5289,
      "longitude": -122.6823,
      "avg_speed_kn": 4.8,
      "max_speed_kn": 5.9,
      "moored": false,
      "engine_on": false,
      "total_distance_nm": 3.1,
      "total_time_ms": 2400000,
      "average_wind_speed_kn": 13.2,
      "average_wind_angle_deg": 48.0,
      "cog_deg": 142.0,
      "average_heading_deg": 140.0
    }
  ],
  "error": null
}
```

**Fields**:
- `timestamp`: Position timestamp (ISO 8601 format)
- `latitude`: Latitude in decimal degrees
- `longitude`: Longitude in decimal degrees
- `avg_speed_kn`: Average speed in knots over the reporting interval
- `max_speed_kn`: Maximum speed in knots over the reporting interval
- `moored`: Boolean indicating if vessel is moored/stationary
- `engine_on`: Boolean indicating if engine is running
- `total_distance_nm`: Cumulative distance from trip start in nautical miles
- `total_time_ms`: Cumulative time from trip start in milliseconds
- `average_wind_speed_kn`: Average apparent wind speed in knots (nullable)
- `average_wind_angle_deg`: Average apparent wind angle in degrees (nullable)
- `cog_deg`: Course over ground in degrees (nullable)
- `average_heading_deg`: Average vessel heading in degrees (nullable)

---

### 4. Get Environmental Metrics

Retrieve environmental sensor data (temperature, pressure, humidity, wind, etc.) with optional filtering.

**Endpoint**: `GET /api/metrics`

**Query Parameters**:
- `metric` (required, string): Metric ID to retrieve (see Metric IDs below)
- `trip_id` (optional, integer): Filter by specific trip
- `start` (optional, string): Start date/time (ISO 8601 format)
- `end` (optional, string): End date/time (ISO 8601 format)

**Metric IDs**:
- `1` - Atmospheric Pressure (Pa)
- `2` - Cabin Temperature (°C)
- `3` - Water Temperature (°C)
- `4` - Humidity (%)
- `5` - Wind Speed (m/s)
- `6` - Wind Direction (degrees)
- `7` - Roll (degrees)

**Examples**:
```bash
# Get cabin temperature for a specific trip
curl "http://localhost:8080/api/metrics?metric=2&trip_id=132"

# Get pressure readings for a date range
curl "http://localhost:8080/api/metrics?metric=1&start=2026-02-01T00:00:00&end=2026-02-03T23:59:59"

# Get wind speed data
curl "http://localhost:8080/api/metrics?metric=5&trip_id=132"
```

**Response**:
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2026-02-02T08:30:00",
      "metric_id": "2",
      "avg_value": 21.5,
      "max_value": 22.1,
      "min_value": 20.9,
      "count": 120
    },
    {
      "timestamp": "2026-02-02T08:40:00",
      "metric_id": "2",
      "avg_value": 21.8,
      "max_value": 22.3,
      "min_value": 21.4,
      "count": 120
    }
  ],
  "error": null
}
```

**Fields**:
- `timestamp`: Measurement timestamp (ISO 8601 format)
- `metric_id`: Identifier of the metric (matches query parameter)
- `avg_value`: Average value over the aggregation period (nullable)
- `max_value`: Maximum value over the aggregation period (nullable)
- `min_value`: Minimum value over the aggregation period (nullable)
- `count`: Number of samples aggregated (nullable)

---

### 5. Update Trip Description

Update the description of a specific trip.

**Endpoint**: `POST /api/trip_description`

**Content-Type**: `application/json`

**Request Body**:
```json
{
  "id": 132,
  "description": "Updated trip description"
}
```

**Fields**:
- `id` (required, integer): Trip ID to update
- `description` (required, string): New description text

**Example**:
```bash
curl -X POST http://localhost:8080/api/trip_description \
  -H "Content-Type: application/json" \
  -d '{"id": 132, "description": "Weekend sail to Marina Bay"}'
```

**Success Response**:
```json
{
  "status": "ok",
  "data": null,
  "error": null
}
```

**Error Response**:
```json
{
  "status": "error",
  "data": null,
  "error": "Trip not found or database error"
}
```

---

## Error Handling

The API uses HTTP status codes and structured error responses:

### HTTP Status Codes
- `200 OK`: Successful request (both success and error responses use 200)
- `500 Internal Server Error`: Unexpected server error

### Common Error Scenarios

**Trip Not Found**:
```json
{
  "status": "error",
  "data": null,
  "error": "Trip 999 not found"
}
```

**Database Connection Error**:
```json
{
  "status": "error",
  "data": null,
  "error": "Database connection error: ..."
}
```

**Invalid Query Parameter**:
```json
{
  "status": "error",
  "data": null,
  "error": "Invalid parameter format"
}
```

---

## Usage Examples

### JavaScript (Fetch API)

```javascript
// Get all trips
async function getTrips() {
  const response = await fetch('http://localhost:8080/api/trips');
  const result = await response.json();
  
  if (result.status === 'ok') {
    console.log('Trips:', result.data);
  } else {
    console.error('Error:', result.error);
  }
}

// Get track for a specific trip
async function getTripTrack(tripId) {
  const response = await fetch(`http://localhost:8080/api/track?trip_id=${tripId}`);
  const result = await response.json();
  
  if (result.status === 'ok') {
    return result.data;
  } else {
    throw new Error(result.error);
  }
}

// Update trip description
async function updateTrip(tripId, description) {
  const response = await fetch('http://localhost:8080/api/trip_description', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json'
    },
    body: JSON.stringify({ id: tripId, description })
  });
  
  const result = await response.json();
  return result.status === 'ok';
}
```

### Python (requests)

```python
import requests

# Get all trips
def get_trips():
    response = requests.get('http://localhost:8080/api/trips')
    result = response.json()
    
    if result['status'] == 'ok':
        return result['data']
    else:
        raise Exception(result['error'])

# Get environmental metrics
def get_cabin_temp(trip_id):
    params = {
        'metric': '2',
        'trip_id': trip_id
    }
    response = requests.get('http://localhost:8080/api/metrics', params=params)
    result = response.json()
    
    if result['status'] == 'ok':
        return result['data']
    else:
        raise Exception(result['error'])

# Update trip description
def update_trip_description(trip_id, description):
    payload = {
        'id': trip_id,
        'description': description
    }
    response = requests.post(
        'http://localhost:8080/api/trip_description',
        json=payload
    )
    result = response.json()
    return result['status'] == 'ok'
```

### cURL

```bash
# Get trips from last 3 months
curl "http://localhost:8080/api/trips?last_months=3" | jq .

# Get specific trip details
curl "http://localhost:8080/api/trip?id=132" | jq .

# Get track data and format with jq
curl "http://localhost:8080/api/track?trip_id=132" | jq '.data[] | {time: .timestamp, lat: .latitude, lon: .longitude}'

# Get pressure data for date range
curl "http://localhost:8080/api/metrics?metric=1&start=2026-02-01T00:00:00&end=2026-02-03T23:59:59" | jq .

# Update trip description
curl -X POST http://localhost:8080/api/trip_description \
  -H "Content-Type: application/json" \
  -d '{"id": 132, "description": "Weekend sail"}' | jq .
```

---

## Data Aggregation

### Track Points
Track points are aggregated based on the `status_report_period` configured in `config.json`:
- **Underway**: Default 10 seconds
- **Moored**: Default 5 minutes

### Environmental Metrics
Environmental metrics are aggregated with configurable intervals per metric type:
- Measurements are collected continuously
- Aggregated values include min, max, and average
- Persistence intervals are metric-specific (configured in application)

---

## Configuration

The web server configuration is in `config.json`:

```json
{
  "web_server": {
    "enabled": true,
    "port": 8080
  }
}
```

**Settings**:
- `enabled`: Enable/disable the web server
- `port`: TCP port for the web server (default: 8080)

---

## CORS and Security

**Note**: The current implementation does not include CORS headers or authentication. For production use, consider:

1. Adding CORS middleware if accessing from web applications
2. Implementing authentication/authorization
3. Using HTTPS/TLS for encrypted communication
4. Rate limiting to prevent abuse

---

## Web Interface

In addition to the REST API, static HTML pages are served:

- **GET `/`**: Redirects to `/trips.html`
- **GET `/trips.html`**: Trip listing and management interface
- **GET `/trip_description_form.html`**: Form for updating trip descriptions

Static files are served from the `static/` directory.

---

## Troubleshooting

### Server Not Starting
- Check if port 8080 is already in use
- Verify `config.json` has `web_server.enabled` set to `true`
- Check application logs for database connection issues

### Empty Data Responses
- Verify the database contains data for the requested parameters
- Check date/time format (must be ISO 8601)
- Ensure trip IDs are valid

### Connection Refused
- Confirm the NMEA Router application is running
- Verify firewall settings allow connections to port 8080
- Check the configured port in `config.json`

---

## Version Information

This documentation corresponds to NMEA Router version with Axum-based web server.

For more information about the database schema, see:
- `schema.sql` - Database structure
- `ENVIRONMENTAL_MONITORING.md` - Environmental metrics details
- `README_DATABASE.md` - Database implementation details
