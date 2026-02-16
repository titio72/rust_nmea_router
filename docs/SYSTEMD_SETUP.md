# Running NMEA Router as a systemd Service

## Prerequisites

1. Build the release version:
   ```bash
   cargo build --release
   ```

2. Ensure the CAN interface is configured and accessible

## Installation Steps

### 1. Copy the service file

```bash
sudo cp nmea_router.service /etc/systemd/system/
```

### 2. Adjust the service file (if needed)

Edit the service file to match your setup:

```bash
sudo nano /etc/systemd/system/nmea_router.service
```

Update these fields if necessary:
- `User` and `Group`: Change from `aboni` to your username
- `WorkingDirectory`: Path to your project directory
- `ExecStart`: Path to the compiled binary

### 3. Reload systemd

```bash
sudo systemctl daemon-reload
```

### 4. Enable the service (start on boot)

```bash
sudo systemctl enable nmea_router.service
```

### 5. Start the service

```bash
sudo systemctl start nmea_router.service
```

## Managing the Service

### Check service status
```bash
sudo systemctl status nmea_router.service
```

### View logs
```bash
# Real-time logs
sudo journalctl -u nmea_router.service -f

# All logs
sudo journalctl -u nmea_router.service

# Logs from today
sudo journalctl -u nmea_router.service --since today

# Last 100 lines
sudo journalctl -u nmea_router.service -n 100
```

### Stop the service
```bash
sudo systemctl stop nmea_router.service
```

### Restart the service
```bash
sudo systemctl restart nmea_router.service
```

### Disable the service (prevent start on boot)
```bash
sudo systemctl disable nmea_router.service
```

## Troubleshooting

### Service fails to start

1. Check the service status and logs:
   ```bash
   sudo systemctl status nmea_router.service
   sudo journalctl -u nmea_router.service -n 50
   ```

2. Verify file paths in the service file are correct

3. Ensure the user has permissions to:
   - Read the config.json file
   - Access the CAN interface (may need to add user to `dialout` or similar group)
   - Write to log directory

### Permission issues with CAN interface

Add your user to the appropriate group (usually `dialout` or create a specific group for CAN access):

```bash
sudo usermod -a -G dialout aboni
```

Or create a udev rule for CAN devices in `/etc/udev/rules.d/99-can.rules`:
```
KERNEL=="can*", GROUP="dialout", MODE="0660"
```

Then reload udev rules:
```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### Database connection issues

Ensure PostgreSQL is running and accessible before the service starts. You can add a dependency in the service file:

```ini
[Unit]
Description=NMEA2000 Router Service
After=network.target postgresql.service
Requires=postgresql.service
```

## Configuration Updates

After modifying `config.json`:

```bash
sudo systemctl restart nmea_router.service
```

## Uninstallation

```bash
# Stop and disable the service
sudo systemctl stop nmea_router.service
sudo systemctl disable nmea_router.service

# Remove the service file
sudo rm /etc/systemd/system/nmea_router.service

# Reload systemd
sudo systemctl daemon-reload
```
