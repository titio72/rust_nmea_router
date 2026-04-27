DROP PROCEDURE IF EXISTS get_trip_legs;

CREATE PROCEDURE get_trip_legs(
  IN p_trip_id        BIGINT,
  IN p_min_leg_min    INT,   -- min underway duration (minutes) to count as a real leg
  IN p_min_moored_gap INT    -- moored gap shorter than this (minutes) is noise mid-leg
)
BEGIN
  -- trip_status: all vessel_status rows for the trip, with island group numbers.
  -- The ROW_NUMBER() - ROW_NUMBER() PARTITION BY trick assigns the same grp value
  -- to every consecutive run of the same is_moored state.
  WITH trip_status AS (
    SELECT
      vs.timestamp,
      vs.is_moored,
      vs.latitude,
      vs.longitude,
      ROW_NUMBER() OVER (ORDER BY vs.timestamp)
        - ROW_NUMBER() OVER (PARTITION BY vs.is_moored ORDER BY vs.timestamp) AS raw_grp
    FROM vessel_status vs
    INNER JOIN trips t
      ON t.id = p_trip_id
     AND vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  -- Duration of each consecutive mooring/underway island.
  raw_islands AS (
    SELECT raw_grp, is_moored,
      TIMESTAMPDIFF(MINUTE, MIN(timestamp), MAX(timestamp)) AS dur_min
    FROM trip_status
    GROUP BY raw_grp, is_moored
  ),
  -- Noise smoothing: a moored island shorter than p_min_moored_gap is relabelled
  -- as underway (sm=0), so it won't split a leg in two.
  smoothed AS (
    SELECT
      ts.timestamp,
      ts.latitude,
      ts.longitude,
      CASE
        WHEN ts.is_moored = 1 AND ri.dur_min < p_min_moored_gap THEN 0
        ELSE ts.is_moored
      END AS sm
    FROM trip_status ts
    INNER JOIN raw_islands ri
      ON ri.raw_grp = ts.raw_grp
     AND ri.is_moored = ts.is_moored
  ),
  -- Re-apply the islands technique on the smoothed signal.
  smoothed_islands AS (
    SELECT timestamp, latitude, longitude, sm,
      ROW_NUMBER() OVER (ORDER BY timestamp)
        - ROW_NUMBER() OVER (PARTITION BY sm ORDER BY timestamp) AS grp
    FROM smoothed
  ),
  -- Aggregate each underway island into a candidate leg.
  legs_raw AS (
    SELECT grp,
      MIN(timestamp) AS leg_start,
      MAX(timestamp) AS leg_end,
      TIMESTAMPDIFF(MINUTE, MIN(timestamp), MAX(timestamp)) AS dur_min,
      COUNT(*)                                               AS records
    FROM smoothed_islands
    WHERE sm = 0
    GROUP BY grp
    -- Drop legs shorter than the minimum threshold (residual noise).
    HAVING TIMESTAMPDIFF(MINUTE, MIN(timestamp), MAX(timestamp)) >= p_min_leg_min
  )
  -- Final output: leg times + start/end coordinates.
  SELECT
    lr.leg_start,
    lr.leg_end,
    lr.dur_min    AS duration_min,
    vs_s.latitude  AS start_lat,
    vs_s.longitude AS start_lon,
    vs_e.latitude  AS end_lat,
    vs_e.longitude AS end_lon,
    lr.records
  FROM legs_raw lr
  INNER JOIN vessel_status vs_s ON vs_s.timestamp = lr.leg_start
  INNER JOIN vessel_status vs_e ON vs_e.timestamp = lr.leg_end
  ORDER BY lr.leg_start;
END
