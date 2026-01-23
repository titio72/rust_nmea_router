# Database Integration

The NMEA2000 Router now writes vessel status reports to a MariaDB/MySQL database.

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
| `average_speed_ms` | DOUBLE | Average speed over last 30 seconds (m/s) |
| `max_speed_ms` | DOUBLE | Maximum speed over last 30 seconds (m/s) |
| `is_moored` | BOOLEAN | TRUE if moored (stable position for 2+ min) |
| `engine_on` | BOOLEAN | TRUE if engine is running |
| `total_distance_m` | DOUBLE | Distance traveled since last report (meters) |
| `total_time_ms` | BIGINT | Time elapsed since last report (milliseconds) |

### Environmental Metrics

Environmental data is collected and persisted on a metric-by-metric basis with configurable intervals. Each metric is calculated with min/max/average values over the collection period.

**Available Metrics:**
- **Pressure** (metric_id=1): Atmospheric pressure in Pascals (Pa)
- **Cabin Temperature** (metric_id=2): Inside cabin temperature in Celsius (°C)
- **Water Temperature** (metric_id=3): Sea water temperature in Celsius (°C)
- **Humidity** (metric_id=4): Relative humidity in percent (%)
- **Wind Speed** (metric_id=5): Wind speed in meters per second (m/s)
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

## Querying Data

### Latest Vessel Status

```sql
SELECT 
    timestamp,
    CONCAT(latitude, '° N, ', longitude, '° E') as position,
    ROUND(average_speed_ms * 1.94384, 2) as avg_speed_knots,
    ROUND(max_speed_ms * 1.94384, 2) as max_speed_knots,
    ROUND(total_distance_m, 1) as distance_meters,
    ROUND(total_distance_m / 1852.0, 3) as distance_nm,
    IF(engine_on, '🟢 ON', '⚫ OFF') as engine,
    IF(is_moored, '⚓ MOORED', '⛵ UNDERWAY') as status
FROM vessel_status 
ORDER BY timestamp DESC 
LIMIT 1;
```

### Average Speed Last Hour

```sql
SELECT 
    ROUND(AVG(average_speed_ms) * 1.94384, 2) as avg_speed_knots,
    ROUND(MAX(max_speed_ms) * 1.94384, 2) as max_speed_knots
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
    ROUND(average_speed_ms * 1.94384, 2) as speed_knots,
    ROUND(total_distance_m, 1) as distance_m,
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
    MAX(CASE WHEN metric_id = 5 THEN ROUND(value_avg * 1.94384, 1) END) as wind_speed_kt,
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
    AVG(CASE WHEN metric_id = 5 THEN value_avg * 1.94384 END) as avg_wind_kt,
    MAX(CASE WHEN metric_id = 5 THEN value_max * 1.94384 END) as max_gust_kt,
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
