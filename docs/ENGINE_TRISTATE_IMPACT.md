# Engine Status Tristate: Impact Assessment

## Executive Summary

**Recommendation**: ⚠️ **FEASIBLE BUT REQUIRES CAREFUL MIGRATION**

Extending engine status from boolean (off/on) to tristate (off/on/unknown) is straightforward from a database perspective but impacts trip classification logic and UI visualization. The change requires schema updates, business logic adjustments, and UI modifications.

---

## 1. DATABASE SCHEMA IMPACT

### Current State
```sql
ALTER TABLE vessel_status 
  MODIFY engine_on BOOLEAN NOT NULL DEFAULT 0;
```
- Data type: `BOOLEAN` (MySQL alias for `TINYINT(1)`)
- Values: 0 (false) or 1 (true)
- Storage: 1 byte per row

### Required Change
```sql
ALTER TABLE vessel_status 
  MODIFY engine_on TINYINT UNSIGNED NOT NULL DEFAULT 2;
```
- Data type: `TINYINT UNSIGNED`
- Values: 0 (off), 1 (on), 2 (unknown)
- Storage: 1 byte per row (no change)
- **Default**: Change to 2 (unknown) instead of 0

### Migration Considerations
- **Size**: No change (1 byte per row)
- **Backward compatibility**: Moderate concern
  - Existing BOOLEAN queries using `engine_on = 1` will still work
  - But they won't match unknown states (value 2)
  - Queries like `if engine_on` will fail in some languages
- **Indexes**: None on engine_on currently, no index migration needed

---

## 2. RUST CODE IMPACT

### Current Type Definition
```rust
// src/db/types.rs
pub engine_on: bool,

// src/vessel_monitor.rs
pub engine_on: bool,

// src/trip.rs
pub fn update(&mut self, 
    distance: f64, 
    time_ms: u64, 
    engine_on: bool,  // ← Will change to u8
    is_moored: bool)
```

### Required Changes

#### 2.1 Define New Enum (Best Practice)
```rust
// src/utilities.rs or new src/engine_status.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EngineStatus {
    Off = 0,
    On = 1,
    Unknown = 2,
}

impl EngineStatus {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => EngineStatus::Off,
            1 => EngineStatus::On,
            _ => EngineStatus::Unknown,
        }
    }
    
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
    
    pub fn is_on(&self) -> bool {
        matches!(self, EngineStatus::On)
    }
    
    pub fn is_off(&self) -> bool {
        matches!(self, EngineStatus::Off)
    }
    
    pub fn is_unknown(&self) -> bool {
        matches!(self, EngineStatus::Unknown)
    }
}
```

#### 2.2 Update Data Structures
**Files to Modify**: ~10 files

| File | Changes | Complexity |
|------|---------|-----------|
| `src/db/types.rs` | Change `engine_on: bool` to `engine_on: u8` or `EngineStatus` | Low |
| `src/vessel_monitor.rs` | Update field type + `is_engine_running()` logic | Low-Medium |
| `src/vessel_status_handler.rs` | Handle 3-state logic in status creation | Medium |
| `src/trip.rs` | Update `Trip::update()` to handle unknown state | Medium |
| `src/db/operations/vessel_status.rs` | Update INSERT/SELECT queries | Low |
| `src/db/operations/query.rs` | Update all query logic | Medium |
| `src/db/operations/import_export.rs` | Handle JSON import/export | Low |
| REST API responses | JSON serialization | Low |

### 2.3 Trip Classification Logic Impact

**Current Logic** (line 43 in trip.rs):
```rust
if is_moored {
    self.total_time_moored += time_ms;
} else if engine_on {
    self.total_distance_motoring += distance;
    self.total_time_motoring += time_ms;
} else {
    self.total_distance_sailed += distance;
    self.total_time_sailing += time_ms;
}
```

**New Logic Options**:

**Option A: Treat Unknown as Not Motoring**
```rust
if is_moored {
    self.total_time_moored += time_ms;
} else if engine_on == EngineStatus::On {
    self.total_distance_motoring += distance;
    self.total_time_motoring += time_ms;
} else {
    // Off OR Unknown → treat as sailing
    self.total_distance_sailed += distance;
    self.total_time_sailing += time_ms;
}
```
- ✅ Pros: Conservative, preserves sailing statistics
- ❌ Cons: Unknown states skew sailing distance upward

**Option B: Treat Unknown as Separate Category**
```rust
pub struct Trip {
    pub total_distance_sailed: f64,
    pub total_distance_motoring: f64,
    pub total_distance_unknown: f64,  // ← NEW
    pub total_time_sailing: u64,
    pub total_time_motoring: u64,
    pub total_time_unknown: u64,      // ← NEW
    pub total_time_moored: u64,
}
```
- ✅ Pros: Accurate reporting, tracks data quality
- ❌ Cons: Schema change, API/UI updates needed, analytics complexity

**Option C: Treat Unknown as Motoring (Conservative)**
```rust
if is_moored {
    self.total_time_moored += time_ms;
} else if engine_on != EngineStatus::Off {
    // On OR Unknown → treat as motoring
    self.total_distance_motoring += distance;
    self.total_time_motoring += time_ms;
} else {
    self.total_distance_sailed += distance;
    self.total_time_sailing += time_ms;
}
```
- ✅ Pros: Errs on side of caution, fuel reporting conservative
- ❌ Cons: Unknown states skew motoring distance upward

**Recommendation**: **Option A** (unknown = sailing) is most practical for existing schema

### 2.4 Engine Status Detection Logic

**Current** (vessel_monitor.rs line 193):
```rust
self.engine_on = engine_msg.is_engine_running();
```

**Enhancement Needed**:
```rust
pub fn get_engine_status(&self, engine_msg: &EngineRapidUpdate) -> EngineStatus {
    // Determine status from RPM (from spec: RPM > 100 = on, ≤ 100 = off)
    match engine_msg.rpm() {
        Some(rpm) if rpm > 100 => EngineStatus::On,
        Some(rpm) if rpm <= 100 => EngineStatus::Off,
        Some(rpm) => EngineStatus::Unknown,  // If RPM value invalid
        None => EngineStatus::Unknown,        // If RPM not available
    }
}
```

---

## 3. DATABASE QUERY IMPACT

### Current Patterns (30+ locations)
```rust
// Pattern 1: Simple boolean check
if engine_on { /* motoring */ }

// Pattern 2: Query with WHERE
WHERE engine_on = 1

// Pattern 3: Speed distribution logic (line 367)
if engine_on != 0 { /* motoring */ }
```

### Required Updates
- **5 queries** in `query.rs` that filter/check `engine_on`
- **5 INSERT/SELECT** operations in `vessel_status.rs`
- **2 import/export** operations in `import_export.rs`
- **2 JSON** serializations for API responses

**Effort**: ~1-2 hours for all updates

---

## 4. API & REST ENDPOINT IMPACT

### Current JSON Response
```json
{
  "timestamp": "2026-02-12T10:30:00Z",
  "latitude": 48.1234,
  "longitude": -4.5678,
  "engine_on": false,
  ...
}
```

### New Response Options

**Option 1: Keep as Boolean (Backward Compatible)**
```json
{
  "engine_on": true,  // 1=true, 0=false, 2=null
  "engine_status": "unknown"  // New field for clarity
}
```

**Option 2: Use String Enum (More Explicit)**
```json
{
  "engine_status": "off"  // or "on", "unknown"
}
```

**Option 3: Use Integer (Matches Database)**
```json
{
  "engine_status": 2  // 0=off, 1=on, 2=unknown
}
```

**Recommendation**: **Option 1** for backward compatibility
- Keep `engine_on` boolean (maps 0→false, 1→true, 2→false)
- Add `engine_status_value` or similar: `0|1|2`
- Or use: `"engine_status": {"value": 2, "name": "unknown"}`

---

## 5. UI/WEB INTERFACE IMPACT

### Current Implementation (trip.html)
```javascript
// Line 1610
if (point2.engine_on) {
    // Use dark grey when engine is on (motoring)
    color = '#888888';
} else {
    // Use speed-based color when sailing
    const speed = point2.avg_speed_kn || point1.avg_speed_kn || 0;
    color = getSpeedColor(speed);
}
```

### Required Changes
```javascript
// Enhanced logic to handle 3 states
function getSegmentColor(engineStatus, speed) {
    if (engineStatus === 1) {
        return '#888888';  // Motoring: dark grey
    } else if (engineStatus === 2) {
        return '#FFD700';  // Unknown: gold/yellow
    } else {
        // Sailing: speed-based gradient
        return getSpeedColor(speed);
    }
}
```

### UI Enhancements Needed

1. **Map Legend Update**
   - Add "Unknown Engine Status" (gold/yellow) color
   - Update color explanation

2. **Trip Summary Display**
   - Currently shows: "Sailing", "Motoring", "Moored" time
   - Could add: "Unknown Status" time if Option B used
   - Or show: "Data Quality: 95% known, 5% unknown"

3. **Speed/Distance Charts**
   - Currently grouped: sailing vs motoring
   - Need decision: where to place unknown segments?
   - Option: Show as third category or combined with sailing

4. **Status Indicators**
   - Add visual indicator for "Unknown" status
   - Tooltip showing "Engine status unknown - check NMEA data"

### Visual Design Suggestions
```css
/* Color scheme for 3-state engine */
.engine-on     { color: #888888; } /* Dark grey */
.engine-off    { color: #1f77b4; } /* Blue (speed-based) */
.engine-unknown { color: #FFD700; } /* Gold */
```

---

## 6. MIGRATION STRATEGY

### Phase 1: Schema & Core Logic (Week 1)
```sql
-- 1. Add new column
ALTER TABLE vessel_status 
  ADD COLUMN engine_status TINYINT UNSIGNED DEFAULT 2;

-- 2. Migrate data (default unknown for now)
UPDATE vessel_status SET engine_status = engine_on;

-- 3. Once validated, drop old column
ALTER TABLE vessel_status 
  DROP COLUMN engine_on;

-- 4. Rename new column
ALTER TABLE vessel_status 
  RENAME COLUMN engine_status TO engine_on;
```

### Phase 2: Code Updates (Week 1)
- Update type definitions and data structures
- Update trip classification logic
- Update database queries
- Add engine status detection enhancement

### Phase 3: API & UI Updates (Week 2)
- Update REST endpoints (JSON serialization)
- Update web interface visualization
- Add legend and tooltips
- Add data quality indicators

### Phase 4: Testing & Validation (Week 2)
- Test trip classifications with mixed data
- Validate historical data interpretations
- Test import/export with new values
- Performance testing (minimal impact expected)

---

## 7. RISK ASSESSMENT

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Trip classification changes | **High** | Use Option A (unknown = sailing), document rationale |
| Backward compatibility | **Medium** | Keep JSON boolean format, add string field for clarity |
| Unknown states too frequent | **Medium** | Track RPM data quality, adjust thresholds if needed |
| Historical data reinterpretation | **Medium** | Document when migrated, allow re-analysis if needed |
| UI display ambiguity | **Low** | Clear color legend, tooltips |

---

## 8. IMPLEMENTATION CHECKLIST

### Core Changes
- [ ] Create `EngineStatus` enum
- [ ] Update `VesselStatus` struct
- [ ] Update `VesselMonitor::get_engine_status()` logic
- [ ] Update `Trip::update()` method
- [ ] Update all database queries (search for `engine_on`)
- [ ] Update import/export JSON handling

### API Changes
- [ ] Update JSON serialization
- [ ] Document API response changes
- [ ] Add backward compatibility note

### UI Changes
- [ ] Update trip.html visualization
- [ ] Add unknown status color to legend
- [ ] Update tooltips and help text
- [ ] Test with sample data

### Testing
- [ ] Unit tests for `EngineStatus` enum
- [ ] Trip classification tests (all 3 states)
- [ ] Database migration test on copy
- [ ] Integration test with historical data
- [ ] UI rendering test for all 3 colors

---

## 9. EFFORT ESTIMATE

| Component | Hours | Complexity |
|-----------|-------|-----------|
| Enum definition | 0.5 | Very Low |
| Type updates | 3 | Low |
| Database migration | 2 | Low |
| Query updates | 3 | Low |
| Trip logic updates | 2 | Medium |
| Engine status detection | 1 | Low |
| API updates | 2 | Low |
| UI changes | 3 | Low |
| Testing | 4 | Medium |
| Documentation | 2 | Low |
| **Total** | **~22 hours** | **Low-Medium** |

---

## 10. RECOMMENDATION

✅ **PROCEED WITH IMPLEMENTATION**

**Reasons**:
1. Aligns with specification requirement (engine status 0/1/2)
2. Improves data quality tracking
3. Minimal performance impact
4. Clear migration path exists
5. UI changes are straightforward

**Suggested Approach**:
1. Use **Option A** for trip classification (unknown = sailing)
2. Use **Enum-based** type system in Rust (cleaner code)
3. Keep JSON response compatible with `engine_on` boolean
4. Add `engine_status_code` field for clarity: `{0, 1, 2}`
5. Update UI with gold/yellow color for unknown states

**Priority**: Medium - Required by spec but not blocking other features
