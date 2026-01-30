# Database Integration

The NMEA2000 Router writes vessel status, environmental data, and trip tracking to a MariaDB/MySQL database with robust resilience features.

## Database Resilience

The application includes comprehensive database reliability features:

### Health Monitoring
- **Periodic Health Checks**: Every 60 seconds, a lightweight `SELECT 1` query verifies database connectivity
- **Automatic Detection**: Connection issues are detected proactively before critical writes
- **Graceful Degradation**: Application continues operating even if database is unavailable

### Automatic Retry Logic
When a database write fails:
1. **Data Retention**: Failed data is kept in memory
2. **Exponential Backoff**: Reconnection attempts use increasing delays (1s, 2s, 4s)
3. **Multiple Attempts**: Up to 3 reconnection attempts per failure
4. **Automatic Retry**: Writes are retried up to 2 times after successful reconnection
5. **No Data Loss**: Transient issues don't cause data loss

### Transaction Atomicity
- **Vessel Status + Trip**: Updates are wrapped in database transactions
- **All or Nothing**: Both operations succeed together or both rollback
- **Data Consistency**: Prevents inconsistent state (e.g., status saved but trip update failed)

### Non-Blocking Operation
- **Socket Timeout**: 500ms CAN read timeout prevents blocking on database operations
- **Continuous Operation**: Metrics, health checks, and monitoring continue regardless of database state

## Database Setup

### 1. Install MariaDB (if not already installed)

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install mariadb-server

# Start MariaDB
sudo systemctl start mariadb
sudo systemctl enable mariadb

# Secure installation (optional but recommended)
sudo mysql_secure_installation
```

### 2. Create Database and User

```bash
sudo mysql
```

Then run:

```sql
CREATE DATABASE nmea_router;
CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';
GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
FLUSH PRIVILEGES;
EXIT;
```

### 3. Create the Table Schema

```bash
mysql -u nmea -p nmea_router < schema.sql
```

Enter password: `nmea`

## Configuration

The application uses the `DATABASE_URL` environment variable for database connection. If not set, it defaults to:

```
mysql://nmea:nmea@localhost:3306/nmea_router
```

### Custom Connection String

Set the `DATABASE_URL` environment variable before running:

```bash
export DATABASE_URL="mysql://username:password@host:port/database"
cargo run
```

Or create a `.env` file (optional, requires adding `dotenv` crate):

```
DATABASE_URL=mysql://nmea:nmea@localhost:3306/nmea_router
```

## Connection String Format

```
mysql://[user]:[password]@[host]:[port]/[database]
```

Examples:
- Local: `mysql://nmea:nmea@localhost:3306/nmea_router`
- Remote: `mysql://user:pass@192.168.1.100:3306/nmea_router`
- With options: `mysql://user:pass@host:3306/db?socket=/var/run/mysqld/mysqld.sock`

## Data Stored

### Vessel Status

Every 30 seconds, the following vessel status data is written to the database:

| Field | Type | Description |
|-------|------|-------------|
| `id` | BIGINT | Auto-incrementing primary key |
| `timestamp` | DATETIME(3) | Report generation time (millisecond precision) |
| `latitude` | DOUBLE | Vessel latitude in decimal degrees (NULL if no fix) |
| `longitude` | DOUBLE | Vessel longitude in decimal degrees (NULL if no fix) |
| `average_speed_kn` | DECIMAL(6,3) | Average speed over reporting period in knots |
| `max_speed_kn` | DECIMAL(6,3) | Maximum speed over reporting period in knots |
| `average_wind_speed_kn` | DECIMAL(6,3) | Average wind speed over reporting period in knots (NULL if no wind data) |
| `average_wind_angle_deg` | DECIMAL(6,3) | Average wind direction over reporting period in degrees (NULL if no wind data) |
| `is_moored` | BOOLEAN | TRUE if moored (stable position for 2+ min) |
| `engine_on` | BOOLEAN | TRUE if engine is running |
| `total_distance_nm` | DOUBLE | Distance traveled since last report in nautical miles |
| `total_time_ms` | BIGINT | Time elapsed since last report (milliseconds) |

### Environmental Metrics

Environmental data is collected and persisted on a metric-by-metric basis with configurable intervals. Each metric is calculated with min/max/average values over the collection period.

**Available Metrics:**
- **Pressure** (metric_id=1): Atmospheric pressure in Pascals (Pa)
- **Cabin Temperature** (metric_id=2): Inside cabin temperature in Celsius (°C)
- **Water Temperature** (metric_id=3): Sea water temperature in Celsius (°C)
- **Humidity** (metric_id=4): Relative humidity in percent (%)
- **Wind Speed** (metric_id=5): Wind speed in Knots (Kn)
- **Wind Direction** (metric_id=6): Wind direction in degrees (°)
- **Roll** (metric_id=7): Vessel roll angle in degrees (°)

**Database Schema:**
```sql
CREATE TABLE environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    metric_id TINYINT UNSIGNED NOT NULL,
    value_avg FLOAT,
    value_max FLOAT,
    value_min FLOAT,
    unit CHAR(3),
    UNIQUE KEY unique_metric_time (timestamp, metric_id)
);
```

Each metric is persisted independently based on configured intervals, allowing efficient storage and querying. For example, wind data might be stored every 10 seconds, while temperature data every 60 seconds.

## Trip Tracking

The system automatically tracks vessel trips, separating sailing time, motoring time, and time at anchor. A trip represents a continuous voyage, with automatic trip boundaries based on inactivity.

**Trip Logic:**
- A new trip is created when vessel status is written and the last trip ended more than 24 hours ago
- An existing trip is updated when vessel status is written within 24 hours of the last trip end
- Each trip records:
  - Start and end timestamps
  - Total distance sailed (engine off, underway)
  - Total distance motoring (engine on, underway)
  - Total time sailing, motoring, and moored
- **Transaction Atomicity**: Vessel status inserts and trip updates are wrapped in a database transaction to ensure both succeed or both rollback together, preventing data inconsistencies

**Database Schema:**
```sql
CREATE TABLE trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL,
    start_timestamp DATETIME(3) NOT NULL,
    end_timestamp DATETIME(3) NOT NULL,
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    total_time_sailing BIGINT NOT NULL DEFAULT 0,
    total_time_motoring BIGINT NOT NULL DEFAULT 0,
    total_time_moored BIGINT NOT NULL DEFAULT 0,
    INDEX idx_end_timestamp (end_timestamp)
);
```

## Querying Data

### Latest Vessel Status

```sql
SELECT 
    timestamp,
    CONCAT(latitude, '° N, ', longitude, '° E') as position,
    ROUND(average_speed_kn, 2) as avg_speed_knots,
    ROUND(max_speed_kn, 2) as max_speed_knots,
    ROUND(total_distance_nm, 3) as distance_nm,
    IF(engine_on, '🟢 ON', '⚫ OFF') as engine,
    IF(is_moored, '⚓ MOORED', '⛵ UNDERWAY') as status
FROM vessel_status 
ORDER BY timestamp DESC 
LIMIT 1;
```

### Average Speed Last Hour

```sql
SELECT 
    ROUND(AVG(average_speed_kn), 2) as avg_speed_knots
FROM vessel_status 
WHERE timestamp > NOW() - INTERVAL 1 HOUR;
```

### Current Trip Summary

```sql
SELECT 
    description,
    start_timestamp,
    end_timestamp,
    ROUND(total_distance_sailed, 2) as distance_sailed_nm,
    ROUND(total_distance_motoring, 2) as distance_motoring_nm,
    ROUND(total_distance_sailed + total_distance_motoring, 2) as total_distance_nm,
    CONCAT(FLOOR(total_time_sailing / 3600000), 'h ', 
           FLOOR((total_time_sailing % 3600000) / 60000), 'm') as time_sailing,
    CONCAT(FLOOR(total_time_motoring / 3600000), 'h ', 
           FLOOR((total_time_motoring % 3600000) / 60000), 'm') as time_motoring,
    CONCAT(FLOOR(total_time_moored / 3600000), 'h ', 
           FLOOR((total_time_moored % 3600000) / 60000), 'm') as time_moored
FROM trips 
ORDER BY end_timestamp DESC 
LIMIT 1;
```

### All Trips Summary

```sql
SELECT 
    id,
    description,
    DATE(start_timestamp) as start_date,
    DATEDIFF(end_timestamp, start_timestamp) as duration_days,
    ROUND(total_distance_sailed + total_distance_motoring, 2) as total_nm,
    ROUND(total_distance_sailed, 2) as sailed_nm,
    ROUND(total_distance_motoring, 2) as motored_nm,
    ROUND(total_distance_sailed / (total_distance_sailed + total_distance_motoring) * 100, 1) as sail_percentage
FROM trips 
ORDER BY start_timestamp DESC;
```

### Latest Environmental Readings

```sql
SELECT 
    ROUND(AVG(average_speed_kn), 2) as avg_speed_knots,
    ROUND(MAX(max_speed_kn), 2) as max_speed_knots
FROM vessel_status 
WHERE timestamp >= NOW() - INTERVAL 1 HOUR;
```

### Mooring Events (Moving → Moored transitions)

```sql
SELECT 
    timestamp,
    CONCAT(latitude, '° N, ', longitude, '° E') as mooring_position
FROM vessel_status v1
WHERE is_moored = TRUE
  AND NOT EXISTS (
      SELECT 1 FROM vessel_status v2 
      WHERE v2.timestamp < v1.timestamp 
        AND v2.timestamp >= v1.timestamp - INTERVAL 5 MINUTE
        AND v2.is_moored = TRUE
  )
ORDER BY timestamp DESC
LIMIT 10;
```

### Track Over Time

```sql
SELECT 
    timestamp,
    latitude,
    longitude,
    ROUND(average_speed_kn, 2) as speed_knots,
    ROUND(total_distance_nm, 1) as distance_nm,
    engine_on,
    is_moored
FROM vessel_status
WHERE timestamp >= NOW() - INTERVAL 24 HOUR
ORDER BY timestamp ASC;
```

### Environmental Data Queries

#### Latest Environmental Readings

```sql
SELECT 
    timestamp,
    MAX(CASE WHEN metric_id = 1 THEN ROUND(value_avg, 0) END) as pressure_pa,
    MAX(CASE WHEN metric_id = 2 THEN ROUND(value_avg, 1) END) as cabin_temp_c,
    MAX(CASE WHEN metric_id = 3 THEN ROUND(value_avg, 1) END) as water_temp_c,
    MAX(CASE WHEN metric_id = 4 THEN ROUND(value_avg, 1) END) as humidity_pct,
    MAX(CASE WHEN metric_id = 5 THEN ROUND(value_avg, 1) END) as wind_speed_kt,
    MAX(CASE WHEN metric_id = 6 THEN ROUND(value_avg, 0) END) as wind_dir_deg,
    MAX(CASE WHEN metric_id = 7 THEN ROUND(value_avg, 1) END) as roll_deg
FROM environmental_data
WHERE timestamp >= NOW() - INTERVAL 1 HOUR
GROUP BY timestamp
ORDER BY timestamp DESC
LIMIT 1;
```

#### Temperature Time Series (Last 24 Hours)

```sql
SELECT 
    timestamp,
    ROUND(value_avg, 1) as avg_temp,
    ROUND(value_max, 1) as max_temp,
    ROUND(value_min, 1) as min_temp
FROM environmental_data
WHERE metric_id = 2  -- Cabin temperature
  AND timestamp >= NOW() - INTERVAL 24 HOUR
ORDER BY timestamp ASC;
```

#### Wind Statistics (Last Hour)

```sql
SELECT 
    AVG(CASE WHEN metric_id = 5 THEN value_avg END) as avg_wind_kt,
    MAX(CASE WHEN metric_id = 5 THEN value_max END) as max_gust_kt,
    AVG(CASE WHEN metric_id = 6 THEN value_avg END) as avg_direction_deg
FROM environmental_data
WHERE metric_id IN (5, 6)  -- Wind speed and direction
  AND timestamp >= NOW() - INTERVAL 1 HOUR;
```

## Troubleshooting

### Connection Failed

If you see `Warning: Failed to connect to database`, check:

1. MariaDB is running: `sudo systemctl status mariadb`
2. Database exists: `mysql -u nmea -p -e "SHOW DATABASES;"`
3. Credentials are correct
4. User has permissions: `mysql -u nmea -p nmea_router -e "SHOW TABLES;"`

### Permission Denied

```sql
GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
FLUSH PRIVILEGES;
```

### Cannot Connect from Remote Host

Update user permissions:

```sql
CREATE USER 'nmea'@'%' IDENTIFIED BY 'nmea';
GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'%';
FLUSH PRIVILEGES;
```

Then configure MariaDB to listen on all interfaces:

```bash
sudo nano /etc/mysql/mariadb.conf.d/50-server.cnf
# Change: bind-address = 0.0.0.0
sudo systemctl restart mariadb
```

## Performance

The application continues to run even if database connection fails. Database writes are non-blocking - if an insert fails, an error is logged but the application continues processing NMEA2000 messages.

With status reports every 30 seconds:
- ~2,880 records per day
- ~86,400 records per month
- ~1,051,200 records per year

Consider adding data retention policies:

```sql
-- Delete records older than 90 days
DELETE FROM vessel_status WHERE timestamp < NOW() - INTERVAL 90 DAY;

-- Or create an event to auto-clean
CREATE EVENT cleanup_old_data
ON SCHEDULE EVERY 1 DAY
DO DELETE FROM vessel_status WHERE timestamp < NOW() - INTERVAL 90 DAY;
```
