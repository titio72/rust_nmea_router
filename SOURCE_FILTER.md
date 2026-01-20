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

## Implementation Details

- The filter is applied after CAN frame assembly but before message-specific processing
- Source addresses are extracted from the NMEA 2000 identifier using `identifier.source()`
- The filter uses a HashMap for O(1) lookup performance
- The configuration is backward compatible - existing configs without `source_filter` will accept all sources
