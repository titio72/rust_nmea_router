/**
 * Shared Theme Management for NMEA Router
 * Handles dark/light theme switching across all pages
 */

const ICON_MAP = {
    'sun':      `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M8 2a5.53 5.53 0 0 0-3.594 1.342c-.766.66-1.321 1.52-1.464 2.383C1.266 6.095 0 7.555 0 9.318 0 11.366 1.708 13 3.781 13h8.906C14.502 13 16 11.57 16 9.773c0-1.636-1.242-2.969-2.834-3.194C12.923 3.999 10.69 2 8 2m0 4a1 1 0 1 1 .001 2A1 1 0 0 1 8 6z" fill="currentColor"/></svg>`,
    'moon':     `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M6 .278a.77.77 0 0 1 .08.858 7.2 7.2 0 0 0-.878 3.46c0 4.021 3.278 7.277 7.318 7.277q.792-.001 1.533-.16a.79.79 0 0 1 .81.316.73.73 0 0 1-.031.893A8.35 8.35 0 0 1 8.344 16C3.734 16 0 12.286 0 7.71 0 4.266 2.114 1.312 5.124.06A.75.75 0 0 1 6 .278"/></svg>`,
    'power':    `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16">
                    <path d="M7.5 1v7h1V1z"/>
                    <path d="M3 8.812a5 5 0 0 1 2.578-4.375l-.485-.874A6 6 0 1 0 11 3.616l-.501.865A5 5 0 1 1 3 8.812"/>
                </svg>`,
    'pencil':   `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M12.854.146a.5.5 0 0 0-.707 0L10.5 1.793 14.207 5.5l1.647-1.646a.5.5 0 0 0 0-.708zm.646 6.061L9.793 2.5 3.293 9H3.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.207zm-7.468 7.468A.5.5 0 0 1 6 13.5V13h-.5a.5.5 0 0 1-.5-.5V12h-.5a.5.5 0 0 1-.5-.5V11h-.5a.5.5 0 0 1-.5-.5V10h-.5a.5.5 0 0 1-.175-.032l-.179.178a.5.5 0 0 0-.11.168l-2 5a.5.5 0 0 0 .65.65l5-2a.5.5 0 0 0 .168-.11z"/></svg>`,
    'download': `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path fill-rule="evenodd" d="M8 0a5.53 5.53 0 0 0-3.594 1.342c-.766.66-1.321 1.52-1.464 2.383C1.266 4.095 0 5.555 0 7.318 0 9.366 1.708 11 3.781 11H7.5V5.5a.5.5 0 0 1 1 0V11h4.188C14.502 11 16 9.57 16 7.773c0-1.636-1.242-2.969-2.834-3.194C12.923 1.999 10.69 0 8 0m-.354 15.854a.5.5 0 0 0 .708 0l3-3a.5.5 0 0 0-.708-.708L8.5 14.293V11h-1v3.293l-2.146-2.147a.5.5 0 0 0-.708.708z"/></svg>`,
    'upload':   `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path fill-rule="evenodd" d="M8 0a5.53 5.53 0 0 0-3.594 1.342c-.766.66-1.321 1.52-1.464 2.383C1.266 4.095 0 5.555 0 7.318 0 9.366 1.708 11 3.781 11H7.5V5.707L5.354 7.854a.5.5 0 1 1-.708-.708l3-3a.5.5 0 0 1 .708 0l3 3a.5.5 0 0 1-.708.708L8.5 5.707V11h4.188C14.502 11 16 9.57 16 7.773c0-1.636-1.242-2.969-2.834-3.194C12.923 1.999 10.69 0 8 0m-.5 14.5V11h1v3.5a.5.5 0 0 1-1 0"/></svg>`,
    'scissors': `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M3.5 3.5c-.614-.884-.074-1.962.858-2.5L8 7.226 11.642 1c.932.538 1.472 1.616.858 2.5L8.81 8.61l1.556 2.661a2.5 2.5 0 1 1-.794.637L8 9.73l-1.572 2.177a2.5 2.5 0 1 1-.794-.637L7.19 8.61zm2.5 10a1.5 1.5 0 1 0-3 0 1.5 1.5 0 0 0 3 0m7 0a1.5 1.5 0 1 0-3 0 1.5 1.5 0 0 0 3 0"/></svg>`,
    'trash':    `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M2.037 3.225A.7.7 0 0 1 2 3c0-1.105 2.686-2 6-2s6 .895 6 2a.7.7 0 0 1-.037.225l-1.684 10.104A2 2 0 0 1 10.305 15H5.694a2 2 0 0 1-1.973-1.671zm9.89-.69C10.966 2.214 9.578 2 8 2c-1.58 0-2.968.215-3.926.534-.477.16-.795.327-.975.466.18.14.498.307.975.466C5.032 3.786 6.42 4 8 4s2.967-.215 3.926-.534c.477-.16.795-.327.975-.466-.18-.14-.498-.307-.975-.466z"/></svg>`,
    'new':      `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16"><path d="M8 2a5.53 5.53 0 0 0-3.594 1.342c-.766.66-1.321 1.52-1.464 2.383C1.266 6.095 0 7.555 0 9.318 0 11.366 1.708 13 3.781 13h8.906C14.502 13 16 11.57 16 9.773c0-1.636-1.242-2.969-2.834-3.194C12.923 3.999 10.69 2 8 2m.5 4v1.5H10a.5.5 0 0 1 0 1H8.5V10a.5.5 0 0 1-1 0V8.5H6a.5.5 0 0 1 0-1h1.5V6a.5.5 0 0 1 1 0"/></svg>`,
    'warn':     `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" class="bi bi-exclamation-triangle" viewBox="0 0 16 16">
                    <path d="M7.938 2.016A.13.13 0 0 1 8.002 2a.13.13 0 0 1 .063.016.15.15 0 0 1 .054.057l6.857 11.667c.036.06.035.124.002.183a.2.2 0 0 1-.054.06.1.1 0 0 1-.066.017H1.146a.1.1 0 0 1-.066-.017.2.2 0 0 1-.054-.06.18.18 0 0 1 .002-.183L7.884 2.073a.15.15 0 0 1 .054-.057m1.044-.45a1.13 1.13 0 0 0-1.96 0L.165 13.233c-.457.778.091 1.767.98 1.767h13.713c.889 0 1.438-.99.98-1.767z"/>
                    <path d="M7.002 12a1 1 0 1 1 2 0 1 1 0 0 1-2 0M7.1 5.995a.905.905 0 1 1 1.8 0l-.35 3.507a.552.552 0 0 1-1.1 0z"/>
                </svg>`,
    'crosshair': `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" class="bi bi-crosshair" viewBox="0 0 16 16">
                    <path d="M8.5.5a.5.5 0 0 0-1 0v.518A7 7 0 0 0 1.018 7.5H.5a.5.5 0 0 0 0 1h.518A7 7 0 0 0 7.5 14.982v.518a.5.5 0 0 0 1 0v-.518A7 7 0 0 0 14.982 8.5h.518a.5.5 0 0 0 0-1h-.518A7 7 0 0 0 8.5 1.018zm-6.48 7A6 6 0 0 1 7.5 2.02v.48a.5.5 0 0 0 1 0v-.48a6 6 0 0 1 5.48 5.48h-.48a.5.5 0 0 0 0 1h.48a6 6 0 0 1-5.48 5.48v-.48a.5.5 0 0 0-1 0v.48A6 6 0 0 1 2.02 8.5h.48a.5.5 0 0 0 0-1zM8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4"/>
                </svg>`,
    'geo':      `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" class="bi bi-geo-alt" viewBox="0 0 16 16">
                    <path d="M12.166 8.94c-.524 1.062-1.234 2.12-1.96 3.07A32 32 0 0 1 8 14.58a32 32 0 0 1-2.206-2.57c-.726-.95-1.436-2.008-1.96-3.07C3.304 7.867 3 6.862 3 6a5 5 0 0 1 10 0c0 .862-.305 1.867-.834 2.94M8 16s6-5.686 6-10A6 6 0 0 0 2 6c0 4.314 6 10 6 10"/>
                    <path d="M8 8a2 2 0 1 1 0-4 2 2 0 0 1 0 4m0 1a3 3 0 1 0 0-6 3 3 0 0 0 0 6"/>
                </svg>`,
    'logout':   `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16">
                    <path fill-rule="evenodd" d="M10 12.5a.5.5 0 0 1-.5.5h-8a.5.5 0 0 1-.5-.5v-9a.5.5 0 0 1 .5-.5h8a.5.5 0 0 1 .5.5v2a.5.5 0 0 0 1 0v-2A1.5 1.5 0 0 0 9.5 2h-8A1.5 1.5 0 0 0 0 3.5v9A1.5 1.5 0 0 0 1.5 14h8a1.5 1.5 0 0 0 1.5-1.5v-2a.5.5 0 0 0-1 0z"/>
                    <path fill-rule="evenodd" d="M15.854 8.354a.5.5 0 0 0 0-.708l-3-3a.5.5 0 0 0-.708.708L14.293 7.5H5.5a.5.5 0 0 0 0 1h8.793l-2.147 2.146a.5.5 0 0 0 .708.708z"/>
                </svg>`
};


/**
 * Initialize theme on page load
 * Restores the user's theme preference from localStorage
 */
function initializeTheme() {
    const savedTheme = localStorage.getItem('theme') || 'light';
    if (savedTheme === 'dark') {
        document.body.classList.add('dark-theme');
        updateThemeButton(true);
    }
    updateBrandLogo(savedTheme === 'dark');
}

/**
 * Update the brand logo based on theme
 * @param {boolean} isDark - True if dark theme is active
 */
function updateBrandLogo(isDark) {
    const brandLogo = document.getElementById('brandLogo');
    if (brandLogo) {
        brandLogo.src = isDark ? '/images/Itaca_v3_dark.svg' : '/images/Itaca_v3.svg';
    }
}

/**
 * Update the theme toggle button appearance
 * @param {boolean} isDark - True if dark theme is active
 */
function updateThemeButton(isDark) {
    const themeIcon = document.getElementById('theme-icon');
    const themeText = document.getElementById('theme-text');
    
    if (themeIcon) {
        themeIcon.innerHTML = isDark ? ICON_MAP['moon'] : ICON_MAP['sun'];
    }
    if (themeText) {
        themeText.textContent = isDark ? 'Light' : 'Dark';
    }
}

/**
 * Toggle between dark and light theme
 * This is the base toggle function - pages can extend it for additional actions
 * @returns {boolean} - True if dark theme is now active
 */
function baseToggleTheme() {
    const isDark = document.body.classList.toggle('dark-theme');
    localStorage.setItem('theme', isDark ? 'dark' : 'light');
    updateThemeButton(isDark);
    updateBrandLogo(isDark);
    return isDark;
}

async function handleLogout() {
    try { await fetch('/api/auth/logout', { method: 'POST', credentials: 'same-origin' }); }
    finally { location.replace('/login.html'); }
}

// Cached UI mode: null = not yet loaded, true/false = read_only value
let _uiReadOnly = null;

async function fetchUiMode() {
    if (_uiReadOnly !== null) return _uiReadOnly;
    try {
        const resp = await fetch('/api/config/read_only', { credentials: 'same-origin' });
        if (resp.ok) {
            const data = await resp.json();
            _uiReadOnly = !!data.read_only;
        } else {
            _uiReadOnly = false;
        }
    } catch (_) {
        _uiReadOnly = false;
    }
    return _uiReadOnly;
}

async function applyUiMode() {
    const readOnly = await fetchUiMode();
    if (readOnly) {
        document.querySelectorAll('[data-ro-hidden]').forEach(el => { el.style.display = 'none'; });
    }
    document.dispatchEvent(new CustomEvent('uiModeLoaded', { detail: { readOnly } }));
}

document.addEventListener('DOMContentLoaded', applyUiMode);

/**
 * Create the common navigation header for all pages
 * @param {string} currentPage - The current page identifier ('trips', 'monitor', 'stats', 'signalk-browser')
 * @param {boolean} includeRealtimeStatus - Optional: include real-time connection status indicator (for monitor page)
 */
function createHeaderBar(currentPage, showConnectionStatus = false) {
    const navItems = [
        { href: '/', label: 'Trips', page: 'trips', roHidden: false },
        { href: '/realtime.html', label: 'Monitor', page: 'monitor', roHidden: true },
        { href: '/ais.html', label: 'AIS', page: 'ais', roHidden: true },
        { href: '/yearly-stats.html', label: 'Stats', page: 'stats', roHidden: false },
        { href: '/signalk-browser.html', label: 'SignalK Browser', page: 'signalk-browser', roHidden: true },
        { href: '/backup.html', label: 'Backup', page: 'backup', roHidden: true }
    ];

    let headerHTML = `
        <div class="header-bar">
            <div style="display: flex; align-items: center; gap: 15px;">
                <img id="brandLogo" src="/images/Itaca_v3.svg" alt="Logo" style="height: 80px; margin-right: 15px;">
                <div>
                    <h2 style="margin: 0; color: var(--text-primary);">NMEA Router</h2>
                    <nav class="navigation-links" style="margin-top: 8px;">`;

    navItems.forEach(item => {
        const isActive = item.page === currentPage ? ' nav-link-active' : '';
        const roAttr = item.roHidden ? ' data-ro-hidden="true"' : '';
        headerHTML += `<a href="${item.href}" class="nav-link${isActive}"${roAttr}>${item.label}</a>`;
    });

    headerHTML += `
                    </nav>
                </div>
            </div>`;


    headerHTML +=
        `<div style="display: flex; align-items: center; gap: 8px; margin-left: auto;">`;

    headerHTML +=
            `<button class="theme-toggle" id="themeBtn" onclick="baseToggleTheme()">
                <span id="theme-icon">${ICON_MAP['sun']}</span><span id="theme-text">Dark</span>
            </button>`;

    if (showConnectionStatus) {
        headerHTML +=
            `<button class="theme-toggle" title="Real-time connection status">
                <span class="status-dot disconnected" id="connectionStatus"></span>
            </button>`;
    }

    headerHTML +=
            `<button class="theme-toggle" onclick="handleLogout()" title="Sign out">
                ${ICON_MAP['logout']}
            </button>`;

    headerHTML +=
            `<button class="theme-toggle shutdown-btn" id="shutdownBtn" onclick="confirmShutdown()" title="Shutdown system" data-ro-hidden="true">
                ${ICON_MAP['power']}
            </button>`;

    headerHTML +=
        `</div>`;

    headerHTML += `
        </div>`;

    return headerHTML;
}

function updateConnectionStatus(connected) {
    const statusDot = document.getElementById('connectionStatus');

    if (connected) {
        statusDot.classList.add('connected');
        statusDot.classList.remove('disconnected');
    } else {
        statusDot.classList.remove('connected');
        statusDot.classList.add('disconnected');
    }
}

// ----------------------- General purpose functions ----------------------

function hslToRgb(h, s, l) {
    h /= 360;
    s /= 100;
    l /= 100;

    const hue2rgb = (p, q, t) => {
        if (t < 0) t += 1;
        if (t > 1) t -= 1;
        if (t < 1/6) return p + (q - p) * 6 * t;
        if (t < 1/2) return q;
        if (t < 2/3) return p + (q - p) * (2/3 - t) * 6;
        return p;
    };

    let r, g, b;

    if (s === 0) {
        r = g = b = l; // achromatic
    } else {
        const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        const p = 2 * l - q;
        r = hue2rgb(p, q, h + 1/3);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 1/3);
    }

    const toHex = c => {
        const hex = Math.round(c * 255).toString(16);
        return hex.length === 1 ? '0' + hex : hex;
    };

    return '#' + toHex(r) + toHex(g) + toHex(b);
}

function formatDateTime(dateStr) {
    if (!dateStr) return 'Unknown';
    try {
        const date = new Date(dateStr);
        return date.toLocaleDateString() + ' ' + date.toLocaleTimeString();
    } catch (e) {
        return dateStr;
    }
}

function formatDate(dateStr) {
    if (!dateStr) return 'Unknown';
    try {
        const date = new Date(dateStr);
        return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: '2-digit' });
        // return date.toLocaleDateString();
    } catch (e) {
        return dateStr;
    }
}

function formatTime(dateStr) {
    if (!dateStr) return '';
    try {
        const date = new Date(dateStr);
        return date.toLocaleTimeString([], {hour: '2-digit', minute: '2-digit'});
    } catch (e) {
        return dateStr;
    }
}

function formatDuration(ms) {
    if (!ms) return '0h 0m';
    const totalMinutes = Math.floor(ms / (1000 * 60));
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return hours + 'h ' + minutes + 'm';
}

function formatDateTime(dateStr) {
    if (!dateStr) return '';
    try {
        const date = new Date(dateStr);
        return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], {hour: '2-digit', minute: '2-digit'});
    } catch (e) {
        return dateStr;
    }
}

function formatDateForAPI(dateStr) {
    if (!dateStr) return '';
    // Handle both ISO format (with or without timezone/milliseconds)
    // Expected format: 2026-02-02T05:55:12.895000Z or 2026-02-02 05:55:12
    if (typeof dateStr !== 'string') return '';
    return dateStr.split('.')[0].replace('T', ' ');
}

function formatDurationLong(startDateStr, endDateStr) {
    if (!startDateStr || !endDateStr) return 'N/A';
    try {
        const startDate = new Date(startDateStr);
        const endDate = new Date(endDateStr);
        const durationMs = endDate - startDate;
        
        if (durationMs < 0) return 'N/A';
        
        const totalSeconds = Math.floor(durationMs / 1000);
        const days = Math.floor(totalSeconds / 86400);
        const hours = Math.floor((totalSeconds % 86400) / 3600);
        const minutes = Math.floor((totalSeconds % 3600) / 60);
        
        const parts = [];
        if (days > 0) parts.push(days + 'd');
        if (hours > 0) parts.push(hours + 'h');
        if (minutes > 0) parts.push(minutes + 'm');
        
        return parts.length > 0 ? parts.join(' ') : '0m';
    } catch (e) {
        return 'N/A';
    }
}

// ----------------------- System Actions & Dialogs -----------------------

/**
 * Show a styled confirmation dialog
 * @param {object} options
 * @param {string} options.icon - Emoji/symbol shown at the top (default 'warn')
 * @param {string} options.title - Dialog title
 * @param {string} options.message - Body text (HTML allowed)
 * @param {string} [options.confirmLabel] - Confirm button label (default 'Confirm')
 * @param {function} options.onConfirm - Called when the user clicks the confirm button
 */
function showConfirm({ icon = 'warn', title, message, confirmLabel = 'Confirm', color = 'currentColor', onConfirm }) {
    const overlay = document.createElement('div');
    overlay.style.cssText = `
        position: fixed; inset: 0; background: rgba(0,0,0,0.6);
        display: flex; align-items: center; justify-content: center;
        z-index: 9999;
    `;

    overlay.innerHTML = `
        <div style="
            background: var(--bg-secondary);
            border: 1px solid var(--border-color);
            border-radius: 10px;
            padding: 32px 36px;
            max-width: 420px;
            width: 90%;
            text-align: center;
            box-shadow: 0 8px 32px rgba(0,0,0,0.4);
        ">
            <div style="color: ${color}; width: 48px; height: 48px; margin: 0 auto 16px;">${getIconSVG(icon, 48)}</div>
            <h3 style="margin: 0 0 10px; color: var(--text-primary); font-size: 20px;">${title}</h3>
            <p style="margin: 0 0 28px; color: var(--text-secondary); font-size: 14px; white-space: pre-line;">${message}</p>
            <div style="display: flex; gap: 12px; justify-content: center;">
                <button id="showConfirmCancelBtn"
                    style="
                        padding: 10px 24px; border-radius: 6px; border: 1px solid var(--border-color);
                        background: var(--bg-tertiary); color: var(--text-primary);
                        cursor: pointer; font-size: 14px;
                    ">Cancel</button>
                <button id="showConfirmOkBtn"
                    style="
                        padding: 10px 24px; border-radius: 6px; border: none;
                        background: #e74c3c; color: white;
                        cursor: pointer; font-size: 14px; font-weight: 600;
                    ">${confirmLabel}</button>
            </div>
        </div>
    `;

    document.body.appendChild(overlay);

    overlay.querySelector('#showConfirmCancelBtn').addEventListener('click', () => overlay.remove());
    overlay.querySelector('#showConfirmOkBtn').addEventListener('click', () => {
        overlay.remove();
        onConfirm();
    });
}

/**
 * Show a confirmation dialog and trigger system shutdown if confirmed
 */
function confirmShutdown() {
    showConfirm({
        icon: 'power',
        color: '#e74c3c',
        title: 'Shutdown System?',
        message: 'This will power off the Raspberry Pi. Are you sure?',
        confirmLabel: 'Shutdown',
        onConfirm: executeShutdown
    });
}

async function executeShutdown() {
    const btn = document.getElementById('confirmShutdownBtn');
    const status = document.getElementById('shutdownStatus');
    btn.disabled = true;
    btn.textContent = 'Shutting down…';
    status.style.display = 'block';
    status.textContent = 'Sending shutdown command…';

    try {
        const response = await fetch('/api/system/shutdown', { method: 'POST' });
        const result = await response.json();
        if (result.status === 'ok') {
            status.textContent = 'Shutdown initiated. The system will power off shortly.';
        } else {
            status.textContent = 'Error: ' + (result.error || 'Unknown error');
            btn.disabled = false;
            btn.textContent = 'Shutdown';
        }
    } catch (e) {
        status.textContent = 'Request failed: ' + e.message;
        btn.disabled = false;
        btn.textContent = 'Shutdown';
    }
}

function getIconSVG(icon_name, size = 16) {
    let res = ICON_MAP[icon_name] || '';
    return res.replace(/width="16"/g, `width="${size}"`).replace(/height="16"/g, `height="${size}"`);
}

class AppIcon extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
        this.render();
    }

    render() {
        const iconName = this.getAttribute('name');
        const fill = this.getAttribute('fill') || 'currentColor';
        const svgContent = getIconSVG(iconName);
        this.shadowRoot.innerHTML = `
            <style>
                :host {
                    display: inline-block;
                    width: 16px;
                    height: 16px;
                }
                svg {
                    width: 100%;
                    height: 100%;
                    fill: ${fill};
                }
            </style>
            ${svgContent}
        `;
    }
}

customElements.define('app-icon', AppIcon);