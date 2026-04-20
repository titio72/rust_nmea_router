Scaffold a new dashboard page for the nmea_router web UI. The user will name the page and describe its purpose; you create the HTML file and wire it into the nav.

## Arguments
$ARGUMENTS contains the page name and purpose, e.g.
  "anchor — shows anchor watch status and alarm radius"

## What to implement

### 1. Create `static/<slug>.html`

Required structure — do not omit any of these elements:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title><Page Title> - NMEA Router</title>
    <link rel="icon" type="image/png" href="/images/nmeasail.png">
    <link rel="stylesheet" href="/shared.css">
    <script src="/js/shared-theme.js"></script>
    <style>
        /* page-specific styles only */
    </style>
</head>
<body>
    <div id="app">
        <!-- header injected by shared-theme.js -->
    </div>
    <div class="level-1-container">
        <!-- page content here -->
    </div>

    <script>
        // Call createHeaderBar with the correct page identifier (see nav list below)
        // and initializeTheme() on DOMContentLoaded
        document.addEventListener('DOMContentLoaded', () => {
            const app = document.getElementById('app');
            app.innerHTML = createHeaderBar('<page-id>');
            initializeTheme();
            // ... page init
        });
    </script>
</body>
</html>
```

Rules:
- Page is 1500 px wide, centered — use `.level-1-container` from `shared.css`.
- Do **not** inline `themeBtn` or `brandLogo` markup — `createHeaderBar()` injects them.
- If the page needs custom theme behaviour, override `toggleTheme()` and call `baseToggleTheme()` first.
- No `console.log()` left in committed code.
- Vanilla JS only — no frameworks, no bundlers.
- Fetch data from `/api/...` endpoints; handle errors visibly (show message in the UI, not just `console.error`).
- All displayed values must use project units: knots, nm, decimal degrees, °C, Pa, %.

### 2. Register the page in the nav (`static/js/shared-theme.js`)

In `createHeaderBar`, add an entry to `navItems`:
```js
{ href: '/<slug>.html', label: '<Nav Label>', page: '<page-id>' },
```

Keep the list in a logical order (Trips → Monitor → AIS → Stats → SignalK Browser → Backup → new page).

### 3. Page identifier conventions
Existing identifiers for reference:
- `'trips'` → `/`
- `'monitor'` → `/realtime.html`
- `'ais'` → `/ais.html`
- `'stats'` → `/yearly-stats.html`
- `'signalk-browser'` → `/signalk-browser.html`
- `'backup'` → `/backup.html`

## Checklist before finishing
- [ ] `createHeaderBar('<page-id>')` called correctly so the nav link highlights on this page
- [ ] `initializeTheme()` called after the header is injected
- [ ] No leftover `console.log` calls
- [ ] Page loads without JS errors (check browser console)
- [ ] Nav entry added to `shared-theme.js`
- [ ] Update `AGENTS.md` — add the new page to the **Web Interface** section with its URL path and a one-line description of what it shows
