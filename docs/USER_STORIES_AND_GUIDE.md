# NMEA2000 Router: User Stories and Business Guide

## Table of Contents
1. [What This Software Does](#what-this-software-does)
2. [User Stories](#user-stories)
3. [Configuration: Business Perspective](#configuration-business-perspective)
4. [User Interface Guide](#user-interface-guide)

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

---

## User Stories

### Story 1: Recreational Sailor Tracking Trips
**As a** recreational sailor  
**I want to** automatically track my sailing trips with breakdown of sailing vs. motoring  
**So that** I can review my performance and see how much sail vs. engine I used

**Acceptance Criteria:**
- System automatically creates a new trip when vessel moves
- Trip aggregates: total distance, sailing distance, motoring distance
- Trip aggregates: sailing time, motoring time, moored time
- Trip data persists across restarts
- Trip visualization shows all historical trips with filtering by year/month

**Implementation:** Automatic trip tracking with 24-hour boundaries, stored in `trips` table

---

### Story 2: Performance Analyst Reviewing Fastest Segments
**As a** competitive sailor or performance analyst  
**I want to** identify my fastest 1nm, 5nm, and 10nm segments from any trip  
**So that** I can study conditions and tactics that led to best performance

**Acceptance Criteria:**
- Dashboard displays fastest segments with timestamps
- Can click segments to highlight on map
- Shows average speed, duration, and wind conditions for each segment
- Segments searchable across all historical data
- Can compare conditions between segments (wind, heading, throttle state)

**Implementation:** Track analytics endpoint calculates fastest segments, web UI highlights segments on map

---

### Story 3: Environmental Researcher Monitoring Conditions
**As an** environmental researcher  
**I want to** collect time-series data for atmospheric pressure, water temperature, wind patterns  
**So that** I can analyze weather trends and anomalies over weeks/months

**Acceptance Criteria:**
- System collects environmental data at configurable intervals
- Data stored with average, min, max statistics
- Can export data as CSV/JSON for analysis
- Dashboard charts show trends over time
- Can zoom to specific date ranges

**Implementation:** Environmental monitoring subsystem with configurable per-metric intervals

---

### Story 4: System Integrator Broadcasting Data
**As an** external system integrator  
**I want to** receive live NMEA2000 data via UDP JSON broadcast  
**So that** I can integrate with weather services, cloud monitoring, or charting software

**Acceptance Criteria:**
- System broadcasts vessel status and track data via configurable UDP port
- Format is valid JSON matching REST API structure
- Broadcast continues even if web interface is disabled
- Can filter broadcasts by data type (vessel status, track, environmental)

**Implementation:** UDP broadcaster component sending JSON packets to configurable IP:port

---

### Story 5: Boat Systems Technician Validating Time Sync
**As a** boat systems technician  
**I want to** ensure database times match NMEA2000 system time  
**So that** I can diagnose time synchronization issues

**Acceptance Criteria:**
- System logs time skew warnings if difference > threshold (default 500ms)
- Can configure acceptable skew threshold
- Can optionally set system time from NMEA2000
- Logs show exact time difference and source information

**Implementation:** Time synchronization protection with configurable threshold

---

### Story 5: Data Analyst Querying Historical Data
**As a** data analyst  
**I want to** query trips, track data, and environmental metrics via REST API  
**So that** I can build custom dashboards and reports

**Acceptance Criteria:**
- REST API provides endpoints for: trips, track data, speed distribution, wind statistics, environmental metrics
- Can filter by date range, trip ID, year, recent months
- Response format is consistent JSON
- API documentation is clear with examples

**Implementation:** Comprehensive REST API with Axum web framework

---

### Story 6: Network Operator Managing Configuration
**As a** network/systems operator  
**I want to** control which NMEA2000 sources are trusted for each data type  
**So that** I can prevent erroneous data from corrupting my database

**Acceptance Criteria:**
- Configuration supports source filtering by PGN
- Can map PGN to trusted source device number
- Invalid sources are logged and rejected
- Configuration changes take effect on restart
- Clear examples provided for common vessel setups

**Implementation:** Source filter configuration with PGN to source device mapping

---

### Story 7: Trip Manager Organizing and Sharing Data
**As a** trip manager or data curator  
**I want to** edit trip descriptions, export trips, and manage trip data  
**So that** I can organize my trips, share specific journeys, and maintain clean records

**Acceptance Criteria:**
- Can rename trip descriptions to be meaningful (e.g., "Trip to Catalina" instead of "Trip 2026-02-02")
- Can delete trips that are erroneous or unwanted
- Can trim trip start/end dates to exclude mooring periods
- Can export trips as JSON files for backup or sharing
- Can import previously exported trips back into database
- All trip editing is reflected immediately in dashboard
- Deleted trips are completely removed from database

**Implementation:** REST API endpoints for trip CRUD operations (update, delete, trim, export, import)

---

### Story 8: Activity Tracker Monitoring Sailing Frequency
**As a** sailing enthusiast or activity tracker  
**I want to** see a visual heatmap showing when I've been active (sailed)  
**So that** I can identify patterns and celebrate consistent sailing habits

**Acceptance Criteria:**
- Dashboard shows GitHub-style heatmap for last 365 days
- Color intensity represents distance traveled each day (green = more activity)
- Can select any end date to view rolling 365-day window
- Statistics show: total days active, max distance in a day, total distance in period
- Tooltips show date and distance when hovering over cells
- Can export activity data for personal tracking

**Implementation:** Heatmap API endpoint and visualization on main dashboard

---

### Story 9: Performance Coach Analyzing Leg-by-Leg Breakdown
**As a** racing sailor or performance coach  
**I want to** break down each trip into individual sailing legs (continuous underway periods)  
**So that** I can analyze performance consistency across multiple legs and identify weaknesses

**Acceptance Criteria:**
- Each trip automatically broken into legs (separated by mooring/inactivity)
- Each leg shows: duration, distance, average speed, wind conditions
- Can compare wind/speed across legs within same trip
- Legs numbered sequentially (Leg 1 of 5, etc.)
- Dashboard navigator allows switching between legs
- Performance metrics calculated per leg, not just trip-wide
- Leg data exported with trip data

**Implementation:** Trip legs API endpoint and leg navigator in trip details UI

---

### Story 10: Historical Analyst Comparing Patterns Year-to-Year
**As a** historical analyst or yacht club statistician  
**I want to** view monthly and yearly statistics comparing multiple years  
**So that** I can identify seasonal patterns and track long-term trends

**Acceptance Criteria:**
- Yearly statistics page shows aggregated data by month
- Can compare multiple years side-by-side
- Shows: total trips per month, total distance, average speeds, sail percentages
- Charts show trends (more sailing in summer, less in winter, etc.)
- Can export monthly summary as CSV for spreadsheet analysis
- Statistics update automatically as new trips are recorded

**Implementation:** Monthly/yearly statistics API endpoint and `yearly-stats.html` page

---

### Story 11: System Operator Controlling Data Collection
**As a** system operator or boat owner  
**I want to** enable/disable tracking and environmental metrics collection  
**So that** I can temporarily pause data collection during storage or maintenance

**Acceptance Criteria:**
- Toggle to pause tracking data collection
- Toggle to pause environmental metrics collection
- Can toggle independently (stop metrics but keep tracking, or vice versa)
- Dashboard shows current collection status
- System continues running, just doesn't persist new data
- No configuration file restart required
- Status persists across application restart

**Implementation:** Tracking status and metrics status API endpoints

---

## Configuration: Business Perspective

### Database Settings

The database persistence strategy directly affects:
- **Data retention cost** (storage volume)
- **System overhead** (CPU, disk I/O)
- **Analysis granularity** (how detailed your historical records are)
- **Battery drain** (for systems on vessel power)

#### Vessel Status Intervals

**What it means:**
- System records vessel position, speed, heading at configurable intervals
- Different intervals apply based on vessel state

**Configuration:**
```json
"database": {
  "vessel_status": {
    "interval_moored_seconds": 1800,      // 30 minutes when stationary
    "interval_underway_seconds": 30       // 30 seconds when moving
  }
}
```

**Business Impact:**

| Setting | Data Granularity | Storage | CPU | Use Case |
|---------|------------------|---------|-----|----------|
| Moored: 1800s (30min) | Position every 30min | Low | Low | Daily cruiser checking on moored boat |
| Moored: 300s (5min) | Position every 5min | Medium | Low | Rental fleet monitoring dock |
| Underway: 30s | Position every 30s | High | Medium | Performance analysis, route replay |
| Underway: 5s | Position every 5s | Very High | High | Racing, detailed path analysis |

**Recommendation by Use Case:**
- **Casual cruisers**: `moored: 1800s, underway: 60s` (minimal overhead)
- **Fleet operators**: `moored: 300s, underway: 30s` (standard monitoring)
- **Performance sailors**: `moored: 300s, underway: 10s` (detailed analysis)
- **Research vessels**: `moored: 60s, underway: 5s` (maximum detail)

#### Environmental Data Intervals

**What it means:**
- System collects environmental metrics (wind, pressure, temperature, etc.)
- Each metric has its own collection interval
- Statistics (avg, min, max) are stored for each interval

**Configuration:**
```json
"database": {
  "environmental": {
    "wind_speed_seconds": 300,        // 5 minutes
    "wind_direction_seconds": 300,    // 5 minutes
    "roll_seconds": 30,               // 30 seconds
    "pressure_seconds": 120,          // 2 minutes
    "cabin_temp_seconds": 300,        // 5 minutes
    "water_temp_seconds": 300,        // 5 minutes
    "humidity_seconds": 300           // 5 minutes
  }
}
```

**Business Impact - Wind Data (most important):**

| Interval | Detail Level | Use Case | Storage per Week |
|----------|--------------|----------|------------------|
| 60s (1min) | High | Race analysis, research | ~4 MB |
| 300s (5min) | Medium | Performance review, weather trends | ~800 KB |
| 600s (10min) | Low | General monitoring | ~400 KB |
| 1800s (30min) | Minimal | Long-term environmental trends only | ~130 KB |

**Business Impact - Roll/Attitude:**

| Interval | Use Case | Notes |
|----------|----------|-------|
| 10s | Stability analysis, sea state research | High resolution motion capture |
| 30s | Comfort monitoring, general sailing dynamics | Standard comfortable precision |
| 60s | Long-term trend analysis | Minimal storage impact |

**Recommendation by Use Case:**
- **Long-distance cruisers**: `wind: 600s, pressure: 300s, temp: 600s, roll: 120s`  
  (Moderate storage, weather trends, comfort baseline)
- **Sailors analyzing performance**: `wind: 300s, pressure: 120s, temp: 300s, roll: 30s`  
  (Detailed wind/weather correlation, tight performance data)
- **Research vessel**: `wind: 60s, pressure: 60s, temp: 60s, roll: 10s`  
  (Maximum detail for meteorological/oceanographic analysis)
- **Fleet monitoring**: `wind: 600s, pressure: 600s, temp: 1800s, roll: 300s`  
  (Minimal overhead, general awareness)

**Key Insight**: Wind intervals are most critical - shorter intervals (300s or less) enable detailed performance analysis, while longer intervals (600s+) are suitable for general awareness.

### Time Synchronization Configuration

**What it means:**
- NMEA2000 system time may drift from computer time
- System validates time before accepting data
- Prevents corrupted timestamps in database

**Configuration:**
```json
"time": {
  "skew_threshold_ms": 500,    // Reject data if time differs > 500ms
  "set_system_time": false     // Don't auto-adjust system clock
}
```

**Business Impact:**
- **Too strict** (100ms): May reject valid data on systems with clock drift
- **Too loose** (5000ms): May accept data with corrupted timestamps
- **Default (500ms)**: Good balance for marine networks with typical drift

**Recommendation:**
- **Most installations**: Keep at `500ms` (default)
- **GPS-synchronized systems**: Can use `200ms` (tighter sync)
- **Systems without GPS**: Use `1000ms` (more tolerance)

### Source Filtering Configuration

**What it means:**
- Different NMEA2000 devices report the same data (e.g., two GPS units)
- System can prefer one source over another
- Prevents conflicting data from different instruments

**Configuration:**
```json
"source_filter": {
  "pgn_source_map": {
    "129025": 22,              // Position: trust only source 22 (Primary GPS)
    "129026": 22,              // COG & SOG: trust only source 22
    "126992": 22,              // System Time: trust only source 22
    "129029": 0                // GNSS Position Data: trust any source
  }
}
```

**Business Impact:**
- **Correct filtering**: Consistent, clean data from trusted instruments
- **No filtering**: May get conflicting position, speed, or time from multiple sources
- **Wrong source number**: All data rejected, system appears to have no position

**Typical Vessel Configurations:**

| Boat Type | GPS | Compass | Autopilot | Config Approach |
|-----------|-----|---------|-----------|-----------------|
| Simple sailboat | 1 GPS | No | No | Accept all sources |
| Cruiser | 1 GPS + backup | Compass | No | Primary GPS only |
| Racing yacht | 2 GPS, compass, AP | Compass | Yes | Primary GPS, compass from autopilot |
| Charter fleet | Fleet GPS, local GPS | Compass, AP | Yes | Fleet GPS primary, local backup |

**Recommendation:**
- **Start with**: Accept all sources, observe data quality
- **Optimize**: Once you know your devices, set preferred sources
- **Critical data**: Position and heading should be from single trusted source

### Web Server Configuration

**What it means:**
- Determines whether web dashboard is available
- Affects port visibility and resource usage
- Independent of data collection (runs regardless)

**Configuration:**
```json
"web": {
  "enabled": true,
  "port": 8080,
  "google_maps_api_key": "your_key_here"
}
```

**Business Impact:**
- **enabled: false**: Data still collected, no web interface, lower resource use
- **enabled: true**: Dashboard available, REST API accessible
- **Port**: 8080 is standard, change if conflicts with other services
- **Google Maps key**: Required for map display; can proceed without (map won't show)

---

## User Interface Guide

### Web Dashboard Overview

The NMEA2000 Router provides a responsive web interface accessible at `http://localhost:8080` (or your configured server).

### 1. Trip Dashboard (Main Page)

**Purpose**: View historical trips and current vessel status

**Layout:**
```
┌─────────────────────────────────────────────────────────────┐
│  NMEA Router  [← Back]                         [◐ Dark Mode] │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Trip Filter:                                                │
│  [All Years ▼]  [Last 3 Months ▼]  [Search: ________]      │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Trip ID │ Date      │ Distance  │ Duration │ Sail %   │ │
│  ├─────────────────────────────────────────────────────────┤ │
│  │ 132     │ 2/2/2026  │ 45.3 NM   │ 8h 15m   │ 71%      │ │
│  │ 131     │ 2/1/2026  │ 52.1 NM   │ 9h 30m   │ 85%      │ │
│  │ 130     │ 1/31/2026 │ 38.7 NM   │ 7h 45m   │ 65%      │ │
│  │ ...     │ ...       │ ...       │ ...      │ ...      │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Click on any trip to view detailed analysis                │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Key Features:**
- **Year Filter**: View trips from specific year
- **Month Filter**: View last N months
- **Trip Summary**: Distance (total, sailing, motoring), duration, percentages
- **Click to View**: Opens detailed trip analysis page

**User Actions:**
1. Filter trips by time period
2. Click trip row to open detailed view
3. Search by trip description (if custom names added)

---

### 2. Trip Details Page

**Purpose**: Analyze a specific trip with maps, charts, and statistics

**Layout:**
```
┌─────────────────────────────────────────────────────────────┐
│  Trip: Marina Bay - 2/2/2026        [← Back to Dashboard]   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Leg Navigator:                                              │
│  [← Prev]  [Leg 1 of 3]  [Next →]                           │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Trip Info:                                                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Start: 8:30 AM  │ End: 4:45 PM  │ Duration: 8h 15m     │ │
│  │ Distance: 45.3 NM (32.1 sailed, 13.2 motored)          │ │
│  │ Time: 71% sailing, 24% motoring, 5% moored             │ │
│  │ Average Speed: 5.5 knots (sailing), 3.2 knots (motor)  │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │                    Google Map                            │ │
│  │                                                          │ │
│  │  [Yellow track line showing route, markers at start/end]│ │
│  │                                                          │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  Performance Analytics:                                      │
│  ┌─────────────┬──────────────┬──────────────┐              │ │
│  │ Max Speed   │ Fastest 1NM  │ Fastest 5NM  │              │ │
│  │ 8.2 kn      │ 7.8 kn       │ 7.5 kn       │              │ │
│  │ 2:30 PM     │ Duration: 8m │ Duration: 40m│              │ │
│  │             │ 2:15-2:23 PM │ 1:45-2:25 PM │              │ │
│  └─────────────┴──────────────┴──────────────┘              │ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Sections:**

#### A. Leg Navigator
- View entire trip OR individual legs (sailing segments)
- Navigate with Previous/Next buttons
- Shows current leg count (e.g., "Leg 1 of 3")
- Each leg is a continuous sailing segment

#### B. Trip Summary
- Start/end times
- Total distance with breakdown
- Time breakdown: sailing%, motoring%, moored%
- Average speeds by mode

#### C. Map View
- Yellow polyline showing track
- Green markers at start and end points
- Click "fastest segment" cards to highlight segments on map
- Orange polyline highlights selected performance segment
- Pan and zoom to explore route details

#### D. Performance Analytics Cards
- **Max Speed**: Highest instantaneous speed (during sailing only)
- **Fastest 1NM**: Average speed for fastest 1 nautical mile segment
- **Fastest 5NM**: Average speed for fastest 5 nautical mile segment
- **Fastest 10NM**: Average speed for fastest 10 nautical mile segment (if available)

**Click on any performance card** to highlight that segment on the map in orange

#### E. Charts (Below main section)

**Speed Chart (SOG - Speed Over Ground)**
- Timeline view of boat speed throughout trip
- X-axis: Time of day
- Y-axis: Speed in knots
- Shows sailing and motoring phases visually

**Heading Chart**
- How the boat's heading changed during trip
- Helps identify turns and course changes
- 0°=North, 90°=East, 180°=South, 270°=West
- Useful for reviewing tactical decisions

**Wind Speed Chart**
- Apparent wind speed over time
- Helps correlate wind conditions with speed
- Higher wind speeds visible as peaks

**Wind Direction Chart** (Relative to Boat)
- 0°=ahead, 90°=abeam, 180°=behind
- Shows how wind angle changed during trip
- Useful for sail trim analysis

**Absolute Wind Direction** (Compass)
- True wind direction (relative to North)
- Helps understand weather patterns
- Compare with heading chart to understand tactics

**Atmospheric Pressure Chart**
- Barometric pressure trends
- May indicate weather changes
- Useful for weather pattern analysis

**Speed Distribution**
- Bar chart showing distance covered at different speed ranges
- Compares sailing vs. motoring in each speed range
- Helps understand efficiency

**Wind Statistics (Polar)**
- Circular chart showing wind direction distribution
- Color indicates wind strength
- Shows prevailing wind direction during trip
- Useful for route planning

**User Actions:**
1. Use leg navigator to examine individual sailing legs
2. Look at speed chart for throttle/trim opportunities
3. Check wind conditions via wind charts
4. Click performance cards to identify best-performing segments
5. Review heading changes for tactical analysis
6. Edit trip description using "Edit" button
7. Delete trip using "Delete" button (with confirmation)
8. Trim trip start/end times using "Trim" button to exclude mooring periods

**Trip Management Features:**
- **Edit Description**: Rename trip to be meaningful (e.g., "Catalina Island Race")
- **Delete Trip**: Permanently remove trip from database
- **Trim Trip**: Adjust start/end timestamps to focus on active sailing periods
- **Export Trip**: Download trip as JSON for backup or external analysis
- All changes reflected immediately on dashboard

---

### 3. Activity Heatmap (Dashboard)

**Purpose**: Visualize sailing frequency and patterns over time

**Location**: Main dashboard page

**Layout:**
```
┌─────────────────────────────────────────────────────────┐
│  Activity Heatmap - Last 365 Days                        │
│                                                          │
│  End Date: [Feb 11, 2026 ▼]                             │
│                                                          │
│  Statistics:                                             │
│  Total: 1,196 NM  │  Active Days: 76  │  Best Day: 57.5 NM
│                                                          │
│  Mon □ □ □ □ □ □ □  (Light = Less Activity)           │
│  Tue □ ■ □ ■ □ ■ □  (Dark = More Activity)             │
│  Wed □ ■ ■ ■ □ ■ □                                     │
│  Thu □ ■ ■ ■ □ ■ □                                     │
│  Fri □ □ □ □ □ ■ □                                     │
│  Sat □ ■ ■ ■ □ ■ ■                                     │
│  Sun □ ■ ■ ■ □ ■ ■                                     │
│       Jan   Feb   Mar   Apr   May   Jun                 │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

**Features:**
- **Date Picker**: Select any end date to view rolling 365-day window
- **Color Scale**: 5-level gradient (white=inactive, dark green=high activity)
- **Tooltips**: Hover over cells to see date and distance
- **Statistics**: Total distance, active days, best day performance
- **Pattern Analysis**: Easily identify seasonal trends (more sailing in summer)

**User Actions:**
1. Change end date to see different time periods
2. Hover over cells to see daily performance
3. Identify patterns (recurring sailing days, seasonal trends)
4. Use as motivation tracker for consistent sailing habits

---

### 4. Yearly Statistics Page

**Purpose**: Analyze trends across months and years

**Access**: "⊕ Yearly Stats" link on main dashboard

**Layout:**
```
┌─────────────────────────────────────────────────────────┐
│  Yearly Statistics                                       │
│                                                          │
│  Compare Year: [2026 ▼]  [2025 ▼]                       │
│                                                          │
│  Month | 2026 | 2025 | % Change |                       │
│  ─────────────────────────────────────                 │
│  Jan   | 234 NM | 198 NM | +18% |                       │
│  Feb   | 189 NM | 156 NM | +21% |                       │
│  Mar   | --- | 287 NM | --- |                           │
│  Apr   | --- | 312 NM | --- |                           │
│  May   | --- | 401 NM | --- |                           │
│  ...   | ... | ... | ... |                              │
│                                                          │
│  Charts:                                                │
│  [Monthly Distance Trend]  [Sailing % by Month]         │
│  [Trips per Month]         [Average Speed by Month]     │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

**Features:**
- **Year Comparison**: Side-by-side view of same months in different years
- **Trend Charts**: Visualize seasonal patterns
- **Growth Metrics**: % change year-over-year
- **Performance Analysis**: Average speeds, sail percentages by month
- **Export Data**: Download as CSV for spreadsheet analysis

**User Actions:**
1. Select years to compare
2. Review monthly trends and patterns
3. Identify best and worst performing months
4. Export data for custom analysis
5. Plan future trips based on seasonal data

---

### 5. Environmental Monitoring Section (if available)

**Purpose**: View environmental conditions over time

**Access**: Usually tab or separate page on dashboard

**Data Shown:**
- Atmospheric pressure trends
- Water temperature (sea state indicator)
- Cabin temperature (comfort indicator)
- Humidity (fog/condensation risk)
- Wind speed/direction trends

**User Actions:**
1. View long-term environmental trends
2. Correlate conditions with trip performance
3. Identify anomalies or interesting weather patterns

---

### 4. REST API Access

**Purpose**: For developers and integrators

**Common Endpoints:**

| Endpoint | Purpose |
|----------|---------|
| `/api/trips` | List all trips, filter by year/months |
| `/api/trip?id=X` | Get specific trip details |
| `/api/track?trip_id=X` | Get position points for trip |
| `/api/speed_distribution?id=X` | Get speed distribution data |
| `/api/wind_statistics?id=X` | Get wind statistics for trip |
| `/api/environmental_metrics?trip_id=X` | Get environmental data |

**Response Format:**
```json
{
  "status": "ok",
  "data": { ... },
  "error": null
}
```

**Common Endpoints:**

| Endpoint | Purpose |
|----------|---------|
| `/api/trips` | List all trips, filter by year/months |
| `/api/trip?id=X` | Get specific trip details |
| `/api/track?trip_id=X` | Get position points for trip |
| `/api/speed_distribution?id=X` | Get speed distribution data |
| `/api/wind_statistics?id=X` | Get wind statistics for trip |
| `/api/trip_legs?id=X` | Get individual legs within a trip |
| `/api/track_analytics?start=...&end=...` | Get fastest segments and performance metrics |
| `/api/heatmap?date=X` | Get activity heatmap for 365 days |
| `/api/monthly_statistics` | Get month-by-month statistics |
| `/api/trip` (POST) | Update trip description |
| `/api/trip` (DELETE) | Delete a trip |
| `/api/trip_trim` (POST) | Adjust trip start/end timestamps |
| `/api/trip_export?id=X` | Export trip as JSON file |
| `/api/trip_import` (POST) | Import previously exported trip |
| `/api/tracking_status` | Get/set whether tracking is enabled |
| `/api/metrics_status` | Get/set whether metrics collection is enabled |

**Response Format:**
```json
{
  "status": "ok",
  "data": { ... },
  "error": null
}
```

**User Actions:**
1. Build custom analysis tools
2. Export data for research
3. Integrate with external platforms
4. Create automated reports
5. Control data collection via API
6. Manage trips programmatically

---

## System Control Features

### Tracking Status Control

**Purpose**: Enable/disable position tracking without restarting application

**Access**: Dashboard control panel or API endpoint

**Options:**
- **Enabled**: System records position and speed continuously
- **Disabled**: System still receives NMEA2000 data but doesn't persist tracking

**Use Cases:**
- Pause collection while boat is in storage (don't record false data)
- Disable before major maintenance (separate old data from new)
- Temporary disable for privacy during dock visits

### Metrics Status Control

**Purpose**: Enable/disable environmental data collection

**Options:**
- **Enabled**: System collects wind, pressure, temperature, humidity, roll data
- **Disabled**: System doesn't collect or persist environmental metrics

**Use Cases:**
- Disable during coastal cruising if environmental detail not needed
- Disable to reduce database size/overhead for casual sailing
- Enable only for research or performance analysis periods

---

## Summary: Putting It All Together

### Quick Start for Different Users:

**Casual Sailor:**
1. Application runs automatically in background
2. Check dashboard to see recent trips
3. View activity heatmap to see sailing patterns
4. Click on any trip to see performance summary
5. Edit trip names to organize and remember special journeys
6. Use map and charts to understand what happened

**Active Sailor:**
1. Configure database intervals for good detail (30 min moored, 30 sec underway)
2. Check heatmap regularly to track sailing frequency
3. Manage trips: edit descriptions, delete duplicates, trim mooring periods
4. After each trip, review performance analytics
5. Export trips for backup or sharing with friends
6. Check yearly stats to see seasonal patterns

**Fleet Manager (if extended to multiple vessels in future):**
1. Configure database intervals for fleet monitoring (30 min moored, 30 sec underway)
2. Set up source filtering for each vessel type
3. Check dashboard regularly for vessel positions
4. Disable tracking temporarily during maintenance periods
5. Use REST API to integrate with fleet tracking system
6. Export monthly statistics for management reporting

**Performance Sailor:**
1. Configure for maximum detail (10s underway intervals)
2. Configure wind intervals to 5 minutes or less for detailed analysis
3. After each trip, review performance analytics
4. Use segment highlighting to understand conditions for best speeds
5. Compare chart data across trips to find patterns
6. Use yearly stats to identify seasonal performance variations
7. Export trip data for video analysis or coaching sessions

**Researcher:**
1. Configure environmental intervals for maximum detail (60s or less)
2. Set short vessel tracking intervals to capture maneuvers
3. Export heatmap data and monthly statistics for analysis
4. Export individual trips for detailed study
5. Use REST API for programmatic access to all data
6. Disable tracking/metrics temporarily during controlled experiments

---

## Troubleshooting Common Issues

**Q: I see "Entire Trip" but want to see individual legs**
- A: Trip needs to have at least 2 sailing segments (continuous underway periods separated by mooring)

**Q: Map shows no track**
- A: Ensure GPS is providing position data (PGN 129025 or 129029)
- A: Check source filtering configuration - may be rejecting GPS source
- A: Verify tracking is enabled (not paused via tracking status control)

**Q: Dashboard shows no data**
- A: Allow 1-2 minutes for system to collect and persist first data point
- A: Check database connection in logs
- A: Verify NMEA2000 bus is providing messages
- A: Check tracking status - may be disabled

**Q: Charts are blank**
- A: Similar to above - check data collection
- A: Ensure specific PGN types are being transmitted (wind, heading, etc.)
- A: Check metrics status - environmental metrics may be disabled

**Q: Trip ends prematurely**
- A: System creates new trip if > 24 hours of inactivity
- A: Database intervals may be too long to capture all activity
- A: Tracking may have been paused

**Q: Heatmap shows no activity**
- A: Check if the date range includes actual sailing trips
- A: Tracking may have been disabled during that period
- A: Try adjusting the end date to a different date range

**Q: Yearly stats page is blank**
- A: Need at least one full month of trips to display statistics
- A: Data from earlier year may not exist yet - select current or recent year

