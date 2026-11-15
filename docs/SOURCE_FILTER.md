# NMEA 2000 Source Filtering

## Overview
The source filter allows you to accept NMEA 2000 messages from specific sources (devices) on a per-PGN basis. This is useful when you have multiple devices broadcasting the same PGN but only want to use data from one authoritative source.

## Configuration

Add a `source_filter` section to your `config.json` file with a `pgn_source_map` object mapping PGN numbers to source addresses:

```json
{
  "can_interface": "vcan0",
  "source_filter": {
    "pgn_source_map": {
      "129025": 22,
      "127488": 5
    }
  },
  ...
}
```

## Behavior

- **With filter**: If a PGN has an entry in the `pgn_source_map`, only messages from the specified source will be accepted. Messages from other sources will be silently dropped.
  
- **Without filter**: If a PGN is not in the `pgn_source_map`, messages from all sources are accepted.

## Examples

### Example 1: Filter GPS position data
If you have multiple GPS units (e.g., source 22 and source 15) but only want to use data from source 22:

```json
{
  "source_filter": {
    "pgn_source_map": {
      "129025": 22,  // Position, Rapid Update - only from source 22
      "129026": 22,  // COG & SOG, Rapid Update - only from source 22
      "129029": 22   // GNSS Position Data - only from source 22
    }
  }
}
```

### Example 2: Filter engine data
If you have engine data coming from multiple sources:

```json
{
  "source_filter": {
    "pgn_source_map": {
      "127488": 5,   // Engine Parameters, Rapid Update - only from source 5
      "127489": 5    // Engine Parameters, Dynamic - only from source 5
    }
  }
}
```

### Example 3: No filtering (default)
To accept messages from all sources, use an empty map or omit the `source_filter` section entirely:

```json
{
  "source_filter": {
    "pgn_source_map": {}
  }
}
```

## Checking the filter

Open the **Bus Explorer** page (`/bus-explorer.html`) to see which devices are on the bus and which PGNs each one sends. Every PGN row is marked as used by the router, dropped by the filter, blocked, or not used. "Blocked" is a PGN configured with source 255 (reject every source): a deliberate choice, so it is shown neutrally and never makes a device "dropped". When a PGN in `pgn_source_map` is silent from its configured source while another source sends it, the page shows a red banner such as "Position, Rapid Update (129025) arrives only from src 41, but the filter expects src 22 — it is being dropped." A configured source of 255 is never reported. Warnings appear once the router has been listening on the bus for 60 s. If one PGN is accepted from two sources at once, its row shows "also accepted from src N". See `docs/REST_API.md` (`GET /api/bus/devices`) for the data behind the page.

## Implementation Details

- The filter is applied after CAN frame assembly but before message-specific processing
- Source addresses are extracted from the NMEA 2000 identifier using `identifier.source()`
- The filter uses a HashMap for O(1) lookup performance
- The configuration is backward compatible - existing configs without `source_filter` will accept all sources
