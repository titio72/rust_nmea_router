# Trip Version-Based Sync — Design

## Background

The boat (`nmea_router` running locally) pushes trip data to a Railway-hosted
"viewer" instance of the same codebase via `POST /api/sync/manifest` and
`POST /api/sync/trip` (`src/web/api.rs`, `src/db/operations/sync.rs`).

The current change-detection design has two independent signals that must
both stay correct:

1. **New trips**: the boat sends all its UUIDs in the manifest; the remote
   reports back which ones it doesn't have (`missing_uuids`).
2. **Changed trips**: the boat separately queries
   `SELECT uuid FROM trips WHERE updated_at > :last_synced_at` and pushes
   anything that comes back, relying on MariaDB's
   `updated_at ... ON UPDATE CURRENT_TIMESTAMP`.

This rotted in production: the local (boat) `trips` table was missing the
`updated_at` column entirely (a documented `ALTER TABLE` migration in
`schema.sql` was never run on this DB). The query failed, the failure was
only logged (`src/web/api.rs:1462-1464`), and sync kept reporting success —
so a trip whose data got longer/updated locally silently never reached
Railway. Separately, `last_synced_at` is a single watermark that advances
unconditionally after every push attempt, so a trip whose push failed for
any reason (network blip, remote rejection) is never retried — the cursor
has already moved past it.

Both failure modes share a root cause: correctness depends on bookkeeping
state (`updated_at`, `last_synced_at`) that has to be perpetually and
correctly maintained by every write path, present and future, with nothing
verifying that it actually is. This design replaces that bookkeeping with a
per-trip version counter that is compared directly between the two systems,
so "does this need to sync" is answered by comparing actual state rather
than trusting a flag someone might have forgotten to set.

Railway traffic is billed, so full trip payloads (which include all
`vessel_status`/`environmental_data` rows) must not be re-sent for unchanged
trips — only the lightweight version comparison should happen on every
sync.

## Schema

Add to `trips` on **both** the boat DB and the Railway DB:

```sql
ALTER TABLE trips ADD COLUMN version BIGINT UNSIGNED NOT NULL DEFAULT 1
  COMMENT 'Bumped on every change to this row; drives remote sync change-detection';
UPDATE trips SET version = 1;
```

No `ON UPDATE CURRENT_TIMESTAMP`-style auto-behavior — the bump is explicit,
via the primitive below, so there is nothing implicit to drift out of sync
with reality.

## The write primitive

All in-place edits to `trips` (live trip creation ticking `end_timestamp`,
trim, mooring-status fix, gap-fill totals recompute, description edit) must
go through one helper that owns the version bump:

```rust
/// The only path allowed to write to `trips` in place. Callers supply the
/// SET fragment for the fields they're changing; this always appends
/// `version = version + 1`, so a call site cannot bump the wrong number of
/// times or forget to bump at all.
fn exec_trip_update(
    tx: &mut impl Queryable,
    set_fragment: &str,
    params: Params,
    trip_id: i64,
) -> Result<(), AppError>
```

Existing call sites building raw `"UPDATE trips SET ..."` statements
(`src/db/operations/trip.rs`, `mooring_fix.rs`, `gap_fill.rs`,
`vessel_status.rs`) are refactored to call this instead of executing their
own SQL directly.

This is a code convention, not a compiler-enforced guarantee — raw SQL run
outside this primitive (e.g. an ad hoc fix) still bypasses it. That gap is
covered by the manual bump below, which is the documented, expected
follow-up for any direct SQL edit (mirroring the existing DB_ANALYST.md
protocol of "any direct edit must end with a trips row touch").

## Manual version bump

A `bump_trip_version(trip_id)` function, exposed as an MCP tool the same way
`fix_mooring_status` is, for direct SQL corrections against `vessel_status`/
`environmental_data` that don't otherwise touch the `trips` row:

```sql
UPDATE trips SET version = version + 1 WHERE id = :id;
```

DB_ANALYST.md's modification protocols get updated to call this after any
manual correction, replacing the current "recompute totals just to touch
the row" workaround.

## `import_trip`: two callers, two version behaviors

`import_trip` (`src/db/operations/import_export.rs:286`) deletes any
existing trip with the same UUID and inserts a fresh row — used both as
the remote's receive-handler for a sync push (`post_sync_trip`) and for
manual/legacy re-imports of an export file. These need different version
semantics:

- **Sync receive**: the boat is authoritative. The remote must adopt the
  boat's version number *exactly*, not increment it — incrementing on
  receipt makes the remote permanently one step ahead of the boat's own
  counter, which silently swallows the *next* single edit from the
  comparison (traced through: boat edits once more → boat version equals
  what remote already reports → looks like "nothing new" even though it
  is).
- **Manual import**: there is no authoritative external version to trust
  (the file may be stale, hand-edited, or from `legacy_import`). Version
  bumps from whatever is already stored locally for that UUID (`existing +
  1`, or `1` for a brand-new UUID).

`import_trip` gains a boolean parameter for this:

```rust
pub fn import_trip(&self, json_data: &str, is_sync: bool) -> Result<i64, AppError>
```

- `is_sync = true` (called from `post_sync_trip`): read `version` from the
  payload JSON, use it verbatim for the inserted row. A payload missing the
  field is a hard error (`AppError`) — sync payloads are always produced by
  `export_trip_to_string` on a version-aware boat, so a missing field means
  a version mismatch between boat and remote code that must not be papered
  over with a guessed value.
- `is_sync = false` (manual import / `legacy_import` / any existing call
  site): compute `version = existing_version_for_uuid.unwrap_or(0) + 1`
  (existing lookup already happens for the delete-before-replace step, so
  this reuses that read), ignoring any `version` field the file happens to
  carry.

The export JSON (`export_trip_to_string`) gains a `"version"` field so
`is_sync = true` imports have something to read.

## Manifest protocol

Replace the UUID-list-plus-timestamp-cursor exchange with a version-map
diff, computed once on the receiving side from data both sides already
have — no cursor state anywhere:

```rust
struct SyncManifestPayload {
    trip_versions: HashMap<String /* uuid */, u64 /* version */>,
}
struct SyncManifestResult {
    deleted_count: usize,
    uuids_to_push: Vec<String>,
}
```

- Boat sends `{uuid: version}` for every local trip (a few KB even at 100+
  trips — negligible on a billed connection). This also replaces
  `all_uuids` for the orphan-deletion step (`delete_trips_not_in_uuids`),
  since the payload's keys are the full UUID set.
- Remote computes `uuids_to_push` = UUIDs missing locally, union UUIDs
  present locally with `local_version < payload_version`. One list, one
  code path — no separate "missing" vs. "modified since" logic.
- Boat exports and sends full trip JSON (unchanged shape, now includes
  `version`) only for `uuids_to_push`.

`last_synced_at` (`system_status`) is no longer read for sync decisions.
It's kept purely for the "last synced at" display in the sync status UI
(`GET /api/sync/status`), set after a successful round-trip.

This design is self-healing: if a push fails partway through, the remote's
stored version for that trip simply stays behind, so the *next* sync's
diff catches it again automatically — no separate retry logic needed, and
no permanent loss the way an advanced cursor could cause today.

## Rollout

Both sides run the same codebase; this is a coordinated two-DB, two-deploy
change (schema migration + code deploy on boat and Railway together). Since
sync is boat-operator-controlled (not scheduled/automatic — confirmed no
cron/timer triggers it), a brief window where the old and new manifest
shapes don't match a mismatched deploy is acceptable and won't corrupt
data; it will just fail closed (deserialization error) until both sides are
upgraded.

## Testing

- Unit tests for `exec_trip_update` bumping version exactly once per call.
- Unit tests for `import_trip(..., is_sync)` covering: new UUID (both
  modes → version 1), existing UUID + `is_sync=false` (existing + 1),
  existing UUID + `is_sync=true` (adopts payload version verbatim,
  including the case where payload version is lower than local — still
  adopted, since sync assumes the boat is authoritative).
- Integration test for the manifest diff: mixed set of new/unchanged/stale
  UUIDs producing the correct `uuids_to_push`.
- Regression test reproducing the original bug: a trip re-imported with
  updated data after an initial sync must appear in the next manifest's
  `uuids_to_push`.
