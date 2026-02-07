# Heatmap Feature Implementation Summary

## Overview
Successfully implemented a `/api/heatmap` endpoint that returns distance traveled grouped by day for the 365 days preceding a given date, formatted similarly to a GitHub commit heatmap.

## Implementation Details

### 1. Database Query (`src/db.rs`)
- **Function**: `fetch_heatmap(end_date: &str)` - New method added to `VesselDatabase` impl block
- **Location**: Lines 1059-1109
- **Parameters**: 
  - `end_date`: Date in YYYY-MM-DD format
- **Returns**: `HeatmapData` struct containing:
  - `days`: Vector of daily distance records
  - `min_distance`: Minimum distance traveled on any day (for visualization scaling)
  - `max_distance`: Maximum distance traveled on any day
  - `total_distance`: Total distance over 365-day period

**SQL Query**:
```sql
SELECT DATE(vs.timestamp) as day, 
       SUM(COALESCE(vs.total_distance_nm, 0)) as total_distance
FROM vessel_status vs
WHERE DATE(vs.timestamp) BETWEEN '{start_date}' AND '{end_date}'
GROUP BY DATE(vs.timestamp)
ORDER BY vs.timestamp
```

### 2. Data Structures (`src/db.rs`)
**HeatmapDay**:
```rust
#[derive(Debug, serde::Serialize)]
pub struct HeatmapDay {
    pub date: String,           // YYYY-MM-DD format
    pub distance_nm: f64,       // Distance in nautical miles
}
```

**HeatmapData**:
```rust
#[derive(Debug, serde::Serialize)]
pub struct HeatmapData {
    pub days: Vec<HeatmapDay>,
    pub min_distance: f64,
    pub max_distance: f64,
    pub total_distance: f64,
}
```

### 3. API Endpoint (`src/web/api.rs`)
- **Route**: `GET /api/heatmap`
- **Query Parameter**: `date` (YYYY-MM-DD format, required)
- **Handler Function**: `get_heatmap()` - Lines 293-305
- **Response Format**: Standard `ApiResponse<HeatmapData>` with JSON serialization

**Example Request**:
```
GET /api/heatmap?date=2026-02-06
```

**Example Response**:
```json
{
  "status": "ok",
  "data": {
    "days": [
      {"date": "2025-06-02", "distance_nm": 0.00051626},
      {"date": "2025-06-06", "distance_nm": 45.2069758899999},
      ...
    ],
    "min_distance": 0.00014972,
    "max_distance": 57.45998304000011,
    "total_distance": 1196.5584995511733
  },
  "error": null
}
```

### 4. Frontend Visualization (`static/heatmap.html`)
Created a GitHub-style heatmap visualization with:
- **Interactive Grid**: 365 cells representing days (7×52 grid layout)
- **Color Intensity Scaling**: 5-level gradient from light (#ebedf0) to dark (#0d3922)
- **Statistics Dashboard**: Shows total distance, max/min daily distances, active days count
- **Tooltips**: Hover over cells to see date and distance traveled
- **Date Picker**: Select any end date to view the preceding 365 days
- **Responsive Design**: Mobile-friendly with flexbox layout

## Testing Results

### Endpoint Verification
✅ Successfully tested with `GET /api/heatmap?date=2026-02-06`

**Response Summary**:
- 76 days with activity in the 365-day period
- Total distance: 1,196.56 nm
- Max daily distance: 57.46 nm (2025-08-15)
- Min daily distance: 0.00015 nm (2025-10-03)

### Compilation Status
✅ Debug build: Compiles successfully
✅ Release build: Compiles successfully with optimizations

## Technical Considerations

### Database Performance
- Query uses indexed `timestamp` field from `vessel_status` table
- GROUP BY on DATE() function efficiently aggregates daily totals
- Date range filtering applied via WHERE clause for optimal query performance

### Timezone Handling
- Uses UTC-explicit database connection (SET time_zone = '+00:00')
- Dates returned in ISO 8601 format (YYYY-MM-DD)
- All timestamps handled consistently across the application

### Error Handling
- Invalid date format returns standard error response
- Database connection errors caught and returned as ApiResponse errors
- Missing data gracefully handled with 0.0 defaults

## API Usage Examples

### Get heatmap for today
```bash
curl "http://localhost:1113/api/heatmap?date=$(date +%Y-%m-%d)"
```

### Get heatmap for a specific date
```bash
curl "http://localhost:1113/api/heatmap?date=2025-12-15"
```

### Access visualization
```
http://localhost:1113/heatmap.html
```

## Files Modified
1. **src/db.rs**
   - Added `HeatmapDay` struct (lines 475-479)
   - Added `HeatmapData` struct (lines 481-486)
   - Added `fetch_heatmap()` method (lines 1059-1109)

2. **src/web/api.rs**
   - Added `HeatmapQuery` struct
   - Added `get_heatmap()` endpoint handler
   - Added `/heatmap` route to `create_api_router()`
   - Updated imports to include `HeatmapData`

3. **static/heatmap.html** (new file)
   - Complete GitHub-style heatmap visualization

## Future Enhancements
- Add export functionality (CSV, PNG)
- Filter by trip or vessel
- Zoom/scroll for mobile view
- Historical comparison (year-over-year)
- Animation on load
