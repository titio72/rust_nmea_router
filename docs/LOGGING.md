# Logging Configuration

The NMEA Router now includes comprehensive logging with daily log file rotation.

## Configuration

Add a `logging` section to your `config.json` file:

```json
{
  "logging": {
    "directory": "./logs",
    "file_prefix": "nmea_router",
    "level": "info"
  },
  ...
}
```

### Configuration Options

- **directory**: Path where log files will be stored (relative or absolute)
  - Default: `./logs`
  - The directory will be created automatically if it doesn't exist

- **file_prefix**: Prefix for log file names
  - Default: `nmea_router`
  - Log files will be named: `{file_prefix}.YYYY-MM-DD`
  - Example: `nmea_router.2026-01-20`

- **level**: Log verbosity level
  - Options: `trace`, `debug`, `info`, `warn`, `error`
  - Default: `info`
  - Can also be overridden with the `RUST_LOG` environment variable

## Log File Rotation

- Log files automatically roll over at midnight (daily rotation)
- Each day gets a new log file with the date appended to the filename
- Old log files are retained (no automatic deletion)

## Examples

### Development Logging
```json
{
  "logging": {
    "directory": "./logs",
    "file_prefix": "nmea_dev",
    "level": "debug"
  }
}
```

### Production Logging
```json
{
  "logging": {
    "directory": "/var/log/nmea_router",
    "file_prefix": "nmea_router",
    "level": "info"
  }
}
```

### Verbose Debugging
```json
{
  "logging": {
    "directory": "./logs",
    "file_prefix": "nmea_debug",
    "level": "trace"
  }
}
```

## Runtime Log Level Override

You can override the configured log level at runtime using the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug ./nmea_router
```

## Log Format

Logs are written in the following format:
```
2026-01-20T12:34:56.789Z INFO NMEA2000 Router - Starting...
2026-01-20T12:34:56.790Z INFO Opening CAN interface: vcan0
2026-01-20T12:34:56.791Z INFO Successfully opened CAN interface: vcan0
```

Each log entry includes:
- Timestamp (RFC3339 format)
- Log level (INFO, WARN, ERROR, DEBUG, TRACE)
- Message
