Add a new REST API endpoint to the nmea_router web server. The user will describe what the endpoint should do; you implement all three required touch-points.

## Arguments
$ARGUMENTS contains a short description of the endpoint to add, e.g.
  "GET /api/foo — returns the latest bar value"

## What to implement

### 1. Query/request struct  (`src/web/api.rs`)
- Add a `#[derive(Debug, Deserialize)]` struct for query parameters (GET) or `#[derive(Debug, Deserialize, Serialize)]` for POST body.
- Name it `<Resource>Query` or `<Resource>Request` following existing conventions.

### 2. Handler function (`src/web/api.rs`)
Follow this exact pattern — no deviation:

```rust
pub async fn <verb>_<resource>(
    State(state): State<AppState>,
    Query(params): Query<XxxQuery>,   // or Json(params): Json<XxxRequest> for POST
) -> Result<Json<ApiResponse<ReturnType>>, StatusCode> {
    match state.db().<db_method>(/* params */) {
        Ok(data) => Ok(Json(ApiResponse::ok(data))),
        Err(e) => {
            error!(error = %e, "Failed to <describe action>");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}
```

Rules:
- Use `state.db()` (read guard helper), never `state.db.read().unwrap()` directly.
- Parse datetimes with `parse_optional_datetime` / `parse_required_datetime` helpers already in the file.
- Return `ApiResponse<T>` wrapping the payload — never raw JSON.
- Log errors with structured `error!(error = %e, "...")` — no `println!`.
- Never call `Utc::now()` inside the handler; timestamps come from query params or DB.

### 3. Route registration (`src/web/api.rs` — `create_api_router`)
Add `.route("/path", get(handler))` (or `post`/`delete`) inside `create_api_router`, grouped near related routes.

## Checklist before finishing
- [ ] No unused imports introduced
- [ ] Units match project rules: knots, nm, decimal degrees, Celsius, Pa, ms
- [ ] If the route returns SI units for SignalK, document the conversion comment
- [ ] `cargo build` passes with no new warnings
- [ ] Update `AGENTS.md` — add the new endpoint to the REST API section under **Web Interface**, with its HTTP method, path, query parameters, and a one-line description of what it returns
