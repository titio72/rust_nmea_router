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
