/**
 * Shared Theme Management for NMEA Router
 * Handles dark/light theme switching across all pages
 */

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
        brandLogo.src = isDark ? '/Itaca_v3_dark.svg' : '/Itaca_v3.svg';
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
        themeIcon.textContent = isDark ? '🌣' : '◐';
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

/**
 * Create the common navigation header for all pages
 * @param {string} currentPage - The current page identifier ('trips', 'monitor', 'stats', 'signalk-browser')
 * @param {boolean} includeRealtimeStatus - Optional: include real-time connection status indicator (for monitor page)
 */
function createHeaderBar(currentPage) {
    const navItems = [
        { href: '/', label: 'Trips', page: 'trips' },
        { href: '/realtime.html', label: 'Monitor', page: 'monitor' },
        { href: '/ais.html', label: 'AIS', page: 'ais' },
        { href: '/yearly-stats.html', label: 'Stats', page: 'stats' },
        { href: '/signalk-browser.html', label: 'SignalK Browser', page: 'signalk-browser' }
    ];

    let headerHTML = `
        <div class="header-bar">
            <div style="display: flex; align-items: center; gap: 15px;">
                <img id="brandLogo" src="/Itaca_v3.svg" alt="Logo" style="height: 40px; margin-right: 15px;">
                <div>
                    <h2 style="margin: 0; color: var(--text-primary);">NMEA Router</h2>
                    <nav class="navigation-links" style="margin-top: 8px;">`;
    
    navItems.forEach(item => {
        const isActive = item.page === currentPage ? ' nav-link-active' : '';
        headerHTML += `<a href="${item.href}" class="nav-link${isActive}">${item.label}</a>`;
    });

    headerHTML += `
                    </nav>
                </div>
            </div>`;


    headerHTML += `
        <button class="theme-toggle" id="themeBtn" onclick="baseToggleTheme()">
            <span id="theme-icon">◐</span> <span id="theme-text">Dark</span>
        </button>`;

    headerHTML += `
        </div>`;

    return headerHTML;
}
