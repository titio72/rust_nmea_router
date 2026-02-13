# NMEA2000 Router: User Stories and Business Guide

## Table of Contents
1. [What This Software Does](#what-this-software-does)
2. [Specs](#specs)
3. [Rules] (#rules)

---

## What This Software Does

### Overview

The **NMEA2000 Router** is a marine vessel data collection and analysis system that listens to NMEA2000 (CAN bus) network messages from your boat's navigation equipment and stores comprehensive information about your vessel's operations, environment, and performance.

### Core Function

The software continuously monitors the NMEA2000 bus (the standard marine networking protocol), processes navigation and environmental data from marine instruments (GPS, depth sounder, wind sensor, thermometer, barometer, etc.), and persists this data to a database for analysis and visualization.

### Key Data Captured

**Navigation & Trip Data:**
- Position (latitude, longitude)
- Speed over ground (SOG) and heading
- Distance traveled while sailing and motoring
- Time spent sailing, motoring, and moored
- Automatic trip segmentation (tracking journeys with 24-hour boundaries)

**Environmental Metrics:**
- Wind speed and direction (true wind calculations)
- Atmospheric pressure (barometric trends)
- Water and cabin temperature
- Humidity levels
- Boat attitude (roll angle)

**System Data:**
- Depth and water speed
- System time synchronization
- Vessel mooring status

### Primary Use Cases

1. **Performance Analysis**: Review how your boat performed during specific trips (average speed, sail vs. motor percentage, fastest segments)
2. **Fleet Management**: Track multiple vessels' movements and environmental conditions
3. **Route Planning**: Analyze historical speed, wind patterns, and performance data for future trips
4. **Environmental Monitoring**: Track water and air conditions over time
5. **Maintenance & Diagnostics**: Monitor system health and performance trends
6. **Data Integration**: Export vessel data to external systems via REST API

## Specs

### NMEA2000
The supported messages are:
1. 130306 Wind speed and direction
2. 126992 System Time
3. 127250 Boat heading
4. 127257 Boat attitude
5. 127488 Engine rapid update
6. 128259 Boat speed through water
7. 129025 Position rapid update
8. 129026 COG and SOG Rapid update
9. 130312 Temperature
10. 130313 Humidity
11. 130314 Pressure
The application constantly monitors the status of the CAN bus, and, in case of failure, start retrying until reconnection. In case of repeated failure, each retry must be at least 5s apart.

NMEA 2000 messages reference is available in pgns.json

### Boat Status Report
By "Status report" we mean the data representing the status of the boat for a certain period of time. A report refers always to a period of time which varies depending on the conditions, in particular, when the boat is moving, the period is shorter (1s to 30s), while it is longer (1 minute to 60 minutes) when the boat is moored or at anchor.
A report is considered valid only if the time synchronization status is "synced" (see below).
A report is also invalid if it didn't accumulate a history of at least 10 position messages during the period.
The report is persisted in the database.
It includes:
1. The position expressed in longitude and latitude with a precision under 1 meter.
2. The timestamp. It represents the timestamp at the moment the report is generated.
3. The duration of period (which typically is the time elapsed since the previous report)
4. The average and maximum SOG (Speed over ground) - typically, it is calculated as the distance between the reported positions and the previously reported position
5. The average COG (Course over ground) - typically, it is calculated as the bearing from the previously reported position and the currently reported position
6. The average heading (this is typically calculated as average of the heading). The heading is always True heading, not magnetic
7. The average wind speed in the period - this is the True Wind speed (TWS)
8. The average wind angle in the period - this is the True Wind angle (TWA), hence the reference is the bow of the boat
9. The total distance traveled in the period. This is typically the distance between the reported position and the previously reported position
10. The engine status - it can be 1 if the engine is on, and 0 if the engine is off. When unknown, it's 2
11. The mooring status - it is 0 when the boat is under way, and 1 when the boat is stationary

#### Calculation of the average COG and SOG
Calculation of average COG and SOG. If a precedent report is available (to be kept in cache in memory for reference), they should be calculated as the distance between the previous report position and the new position divided by the time elapsed from the previous report. The average COG instead is the bearing between the previous position and the new one.
If a previous report is not available, use the average of the SOG and COG received from the bus.

#### What position must be in the report
The position reported in the report is:
1. The last position received from the NMEA 2000 bus if the boat is underway (mooring status is 0)
2. The median position within the period in case the mooring status is 1

#### Recovery of the last Boat Status Report from the database.
As most calculations depend on the previous report, the application is at risk of producing inconsistent or incomplete data in case of restart, as the previous report is lost and no longer in memory.
For this reason, as soon as the application starts, it should load in memory the last boat status report from the database, but only if it is not older than 6 hours (we do not expect the application to stay down for more than 6 hours).
In this way, the total time, the total distance, and average COG and SOG will be calculated correctly even if the application bounces.

### Time synchronization
The application receives NMEA 2000 messages 126992 and compares the timestamp received with the message with the system time. If the skew is larger than 1 second, the application will try to set the system using the timestamp received with NMEA 2000 message.
Two global states are continuously updated in memory and available to all the subsystems of the application: the time skew in milliseconds and the boolean status indicating if it is synced up (i.e. skew < 1000ms).

### Data collection and Boat Status Report generation
The application uses NMEA messages:
1. 130306 Wind speed and direction
2. 127250 Boat heading
3. 127488 Engine rapid update
4. 129025 Position rapid update
5. 129026 COG and SOG Rapid update
The application collects the relevant values in queues storing the pairs (timestamp at the time the message is received, value). The queue is a time bound and rolling and maintains samples for a time window that is the maximum of the underway period (see below) and moored period.
Depending on the engine status the application determines the period of the report. The duration of the period is configurable by the user in a JSON configuration file. In particular, the underway period will be in the JSON attribute "underway_period_ms" and the moored period in "moored_period_ms". The defaults are respectively 30000ms and 300000ms.
When the period expires the report is generated as described in "Boat Status Report" and stored in the database.
The reported position is the median position of the period in case of mooring status = 1, and the last tracked position in case mooring status = 0

#### Engine status
The engine status (0/1) is determined as 1 is the engine's RPM are > 100 and 0 if the engine RPM are <= 100

#### Mooring status
The mooring status is determined by calculating the median position of the last 180 seconds, and it's 1 if X% of the last 180 seconds' positions are within 50 meters from the median position.
X is to be determined in the field, but expected to be in the range 70 to 90.
In alternative (ti be observed in the field which one is more accurate), the check is performed on the speed over the 180s period, instead of position.

#### Wind data collection
The wind data is received through PGN 130306 frm the bus. Only apparent wind messages are considered for the Boat Status Report. The application will automatically convert the apparent wind to true wind using the last received SOG.
In other words, the application will filter out the true wind, coming from the bus, but it will convert the apparent into true using the "rapid update sog" received with 129026 to calculate the true wind.
Only the calculated true wind will queued up for processing.

#### Heading data colleciton
The heading is received from the bus with 127250 PGN. The application will accept only the Magnetic heading (consider unreliable the true heading coming from the bus) and apply the WMM 2025 declination to obtain the true heading.
To calculate the declination, use the last received position from the bus.
The conversion must be applied before the heading is queued up. If the position is not available, the heading is discarded and not added to the queue for processing.

### Trips
A trip is an expedition that can last a few hours or days. A leg of a trip indicates a period of navigation between two moorings.
We consider legs belonging to the same trip if the start of a leg is less than 24 hours from the end of the preceding leg.
The application automatically calculates trips and, for every Boat Status Report, updates the persistent image of the trip in the database reporting:
1. The start timestamp of the trip (this is written once and never updated)
2. The end timestamp (this is updated for each written Boat Status Report)
3. The total time of the trip, and the breakdown into time sailing, time motoring, and time moored
4. The total distance sailed and motored

## Logging
The logs are written on daily files in a folder configurable (default is the application folder).
It must report all the operations (database read/write operations, open and close of ports/sockets/database).
Every 30 seconds, it traces a report with the number of NMEA messages received (broken down by type, and whether or not they succeeded to parse), the number of records written in the database of each type, and the time sync status.
The output is expected to be in the console as well.

#### Periodic stats
Every 30 seconds, the following stats are to be reported:
1. Number of messages received and parsed successfully for each supported PGN (ignore not supported messages)
2. Number of messages failed to parse for each supported PGN (ignore unsupported PGN)
3. Number of records written for each type
4. Time synchronization status (last skew in milliseconds, and sync status)
4. Time synchronization (last skew, and sync status)

## Rules

Coding convention:
1. Backend is written in Rust
2. Frontend is html and javascript
3. Use underscore _ to separate words in function names
4. Use camel notation for struct names
5. Never use now() in function, unless the function is a handler that generates an event (for example, when a NMEA 2000 message is received, it's legit to use now() to generate the timestamp of the event. If a function in invoked because an event is generated, it will have the timestamp as parameter)
6. Configuration fields are read only - any status that under control of the application is to be read and written from the database
All the code, AI or human generated, must follow this rules.
7. All the timestamps are in UTC
8. All the durations are milliseconds
9. Longitude and latitude are in decimal degrees and with the precision of 1 meter or better
10. The speeds are in Knots and distances in nautical miles
11. Temperatures are in Celsius
12. Barometric pressure is in Pa
13. Humidity is in percentage
14. Average angles are calculated by averaging sin(angle) and cos(angle), and calculating the atan of the average sin and average cos
15. For other measures, the preferred aggregation is the median, unless it's too expensive and we can revert to average
16. Distances between two locations are calculated using the Haversine formula
17. Bearing from a position to another is calculated using the Haversine formula
18. Angles are in decimal degrees and normalized to 0..360 degrees range
19. NMEA2000 messages use different units. The structure and classes representing NMEA2000 messages will expose accessors to read the values in the original units (the unit must be explicit in the name of the accessor, like get_angle_radiants), and will expose additional accessors to read values in the application units (like: get_angle_degrees)
20. The application relies on a MySQL database for persistence
21. Conversion from magnetic angles and true angles are performed using WMM 2025