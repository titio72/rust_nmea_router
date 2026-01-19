# Environmental Monitoring

This feature collects environmental data from NMEA2000 bus at 1-minute intervals and stores statistical summaries in the database.

## Monitored Metrics

The system monitors the following environmental parameters:

1. **Atmospheric Pressure** (PGN 130314)
   - Measured in Pascals (Pa)
   - Typical values: 95,000 - 105,000 Pa

2. **Cabin Temperature** (PGN 130312, Instance 0, Source 4)
   - Measured in Celsius (°C)
   - Instance 0 is typically the cabin temperature

3. **Water Temperature** (PGN 130312, Instance 0, Source 0)
   - Measured in Celsius (°C)
   - Sea water temperature

4. **Humidity** (PGN 130313)
   - Measured in percent (%)
   - Range: 0-100%

5. **Wind Speed** (PGN 130306)
   - Measured in meters per second (m/s)
   - Also displayed in knots (kt) for convenience

6. **Wind Direction** (PGN 130306)
   - Measured in degrees (°)
   - Range: 0-360°

7. **Roll Angle** (PGN 127257)
   - Measured in degrees (°)
   - Boat attitude roll angle (port/starboard tilt)

## Sampling and Storage

- **Sample Rate**: Continuous monitoring of all incoming NMEA2000 messages
- **Report Interval**: Every 60 seconds (1 minute)
- **Statistics Stored**: Average, Maximum, and Minimum values for each metric over the 1-minute interval

## Database Schema

The data is stored in the `environmental_data` table using a time-series structure with one row per metric per time interval:

```sql
CREATE TABLE environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    metric_id TINYINT UNSIGNED NOT NULL,
    value_avg DOUBLE,
    value_max DOUBLE,
    value_min DOUBLE,
    unit VARCHAR(10),
    
    INDEX idx_timestamp (timestamp),
    INDEX idx_metric_timestamp (metric_id, timestamp),
    UNIQUE KEY unique_metric_time (timestamp, metric_id)
);
```

### Metric IDs (Enumeration)

The `metric_id` field uses compact integer values for efficient storage and indexing:

- `1` = pressure - Atmospheric pressure (Pa)
- `2` = cabin_temp - Cabin temperature (°C)
- `3` = water_temp - Water temperature (°C)
- `4` = humidity - Relative humidity (%)
- `5` = wind_speed - Wind speed (m/s)
- `6` = wind_dir - Wind direction (degrees)
- `7` = roll - Roll angle (degrees)

Each 1-minute report generates up to **7 rows** (one per metric) with the same timestamp.

## Setup

1. Create the database table:
   ```bash
   mysql -u nmea -p nmea_router < create_environmental_table.sql
   ```

2. Ensure your NMEA2000 network provides the required PGNs:
   - PGN 130314 (Actual Pressure) for atmospheric pressure
   - PGN 130312 (Temperature) for cabin and water temperature
   - PGN 130313 (Humidity) for relative humidity
   - PGN 130306 (Wind Data) for wind speed and direction
   - PGN 127257 (Attitude) for roll angle

3. Run the application:
   ```bash
   cargo run
   ```

## Output

The application displays environmental reports every minute:

```
╔═══════════════════════════════════════════════════════════════╗
║         ENVIRONMENTAL DATA REPORT (1-minute average)         ║
╠═══════════════════════════════════════════════════════════════╣
║  Pressure:   Avg: 101325 Pa  Max: 101400 Pa  Min: 101250 Pa
║  Cabin Temp: Avg: 22.5°C  Max: 23.0°C  Min: 22.0°C
║  Humidity:   Avg: 55.2%  Max: 58.0%  Min: 52.5%
║  Wind Speed: Avg: 5.2 m/s  Max: 7.1 m/s  Min: 3.5 m/s
║              Avg: 10.1 kt   Max: 13.8 kt   Min: 6.8 kt
║  Wind Dir:   Avg: 225°  Max: 250°  Min: 200°
╚═══════════════════════════════════════════════════════════════╝
```

## Querying Data

Example queries using the enumerated metric IDs:

### Last 24 hours of data (pivot format)
```sql
SELECT 
    timestamp,
    MAX(CASE WHEN metric_id = 1 THEN ROUND(value_avg, 0) END) as pressure_pa,
    MAX(CASE WHEN metric_id = 2 THEN ROUND(value_avg, 1) END) as cabin_temp_c,
    MAX(CASE WHEN metric_id = 3 THEN ROUND(value_avg, 1) END) as water_temp_c,
    MAX(CASE WHEN metric_id = 4 THEN ROUND(value_avg, 1) END) as humidity_pct,
    MAX(CASE WHEN metric_id = 5 THEN ROUND(value_avg * 1.94384, 1) END) as wind_speed_kt,
    MAX(CASE WHEN metric_id = 6 THEN ROUND(value_avg, 0) END) as wind_dir_deg
FROM environmental_data
WHERE timestamp >= NOW() - INTERVAL 24 HOUR
GROUP BY timestamp
ORDER BY timestamp DESC;
```

### Specific metric time series (cabin temperature)
```sql
SELECT 
    timestamp,
    ROUND(value_avg, 1) as avg,
    ROUND(value_max, 1) as max,
    ROUND(value_min, 1) as min,
    unit
FROM environmental_data
WHERE metric_id = 2  -- cabin_temp
  AND timestamp >= NOW() - INTERVAL 24 HOUR
ORDER BY timestamp;
```

### Temperature statistics for the day
```sql
SELECT 
    DATE(timestamp) as date,
    ROUND(AVG(value_avg), 1) as avg_temp,
    ROUND(MAX(value_max), 1) as max_temp,
    ROUND(MIN(value_min), 1) as min_temp
FROM environmental_data
WHERE metric_id = 2  -- cabin_temp
  AND timestamp >= CURDATE()
GROUP BY DATE(timestamp);
```

### Wind statistics for the last hour
```sql
SELECT 
    ROUND(AVG(value_avg) * 1.94384, 1) as avg_wind_kt,
    ROUND(MAX(value_max) * 1.94384, 1) as max_gust_kt
FROM environmental_data
WHERE metric_id = 5  -- wind_speed
  AND timestamp >= NOW() - INTERVAL 1 HOUR;
```

### All metrics for a specific time
```sql
SELECT 
    metric_id,
    ROUND(value_avg, 2) as avg,
    ROUND(value_max, 2) as max,
    ROUND(value_min, 2) as min,
    unit
FROM environmental_data
WHERE timestamp = '2026-01-19 14:30:00'
ORDER BY metric_id;
```

## Notes

- If a metric is not available (sensor not connected or no data received), the corresponding fields will be NULL
- All timestamps are stored in UTC timezone
- The system continues to operate even if the database is unavailable, but data will not be logged
