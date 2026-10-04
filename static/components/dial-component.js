/**
 * Dial Component Module
 * Provides custom elements for displaying wind and heading directions on SVG-based compass dials
 */

/**
 * Build SVG compass rose template programmatically
 * Returns an SVG element with compass markings and cardinal directions
 */
function buildCompassRoseSVG() {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttributeNS(null, "width", "210");
    svg.setAttributeNS(null, "height", "210");
    svg.setAttributeNS(null, "style", "display: block; margin: auto;");

    // Get theme-aware colors
    const bgColor = getComputedStyle(document.documentElement).getPropertyValue('--bg-secondary') || '#EEEEFF';
    const tickColor = getComputedStyle(document.documentElement).getPropertyValue('--text-secondary') || '#000000';

    // Circle background
    const circle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    circle.setAttributeNS(null, "cx", "50%");
    circle.setAttributeNS(null, "cy", "50%");
    circle.setAttributeNS(null, "r", "90");
    circle.setAttributeNS(null, "stroke", "var(--border-color)");
    circle.setAttributeNS(null, "stroke-width", "3");
    circle.setAttributeNS(null, "fill", bgColor.trim());
    svg.appendChild(circle);

    // Cardinal direction marker colors (red for cardinal, cyan for intercardinal)
    const paths = [
        // Cardinal markers (0°, 90°, 180°, 270°)
        { fill: "#A0A0A0", angle: 0 },
        { fill: "#A0A0A0", angle: 90 },
        { fill: "#A0A0A0", angle: 180 },
        { fill: "#A0A0A0", angle: 270 },
        // Intercardinal markers (45°, 135°, 225°, 315°)
        { fill: "#A0C0C0", angle: 45 },
        { fill: "#A0C0C0", angle: 135 },
        { fill: "#A0C0C0", angle: 225 },
        { fill: "#A0C0C0", angle: 315 }
    ];

    paths.forEach(({ fill, angle }) => {
        const path1 = document.createElementNS("http://www.w3.org/2000/svg", "path");
        path1.setAttributeNS(null, "d", "M95 95 L105 20 L105 105 Z");
        path1.setAttributeNS(null, "fill", fill);
        path1.setAttributeNS(null, "transform", `rotate(${angle} 105 105)`);
        svg.appendChild(path1);

        const path2 = document.createElementNS("http://www.w3.org/2000/svg", "path");
        path2.setAttributeNS(null, "d", "M105 105 L105 20 L115 95 Z");
        path2.setAttributeNS(null, "fill", fill === "#A0A0A0" ? "#C0C0C0" : "#C0C0C0");
        path2.setAttributeNS(null, "transform", `rotate(${angle} 105 105)`);
        svg.appendChild(path2);
    });

    // Tick marks
    for (let angle = 0; angle < 360; angle += 15) {
        const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
        line.setAttributeNS(null, "d", angle % 45 === 0 ? "M25 105 L45 105" : "M30 105 L40 105");
        line.setAttributeNS(null, "fill", "none");
        line.setAttributeNS(null, "stroke", tickColor.trim() || "black");
        line.setAttributeNS(null, "stroke-width", angle % 45 === 0 ? "3" : "1");
        line.setAttributeNS(null, "transform", `rotate(${angle} 105 105)`);
        svg.appendChild(line);
    }

    return svg;
}

customElements.define('my-compass-rose', class extends HTMLElement {

    static get observedAttributes() {
        return ['angle'];
    }

    constructor() {
        super();
        this.arrow = null;
    }

    connectedCallback() {
        this.render();
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (oldValue !== newValue && this.arrow) {
            this.updateArrows();
        }
    }

    refreshTheme() {
        this.render();
    }

    render() {
        this.innerHTML = '';
        const svg = buildCompassRoseSVG();

        // Arrow (blue)
        this.arrow = document.createElementNS("http://www.w3.org/2000/svg", "path");
        this.arrow.setAttributeNS(null, "d", "M95 105 L105 45 L115 105 Z");
        this.arrow.setAttributeNS(null, "fill", "blue");
        this.arrow.setAttributeNS(null, "stroke", "gray");
        this.arrow.setAttributeNS(null, "stroke-width", "3");
        svg.appendChild(this.arrow);

        this.appendChild(svg);
        this.updateArrows();
    }

    updateArrows() {
        const angle = parseFloat(this.getAttribute('angle')) || 0;

        if (this.arrow) {
            this.arrow.setAttributeNS(null, "transform", `rotate(${angle} 105 105)`);
        }
    }
});



/**
 * Custom element: my-wind-dial
 * Displays true wind angle (TWA) and apparent wind angle (AWA) on a compass
 * Attributes: angle_true (TWA in degrees), angle_app (AWA in degrees)
 */
customElements.define('my-wind-dial', class extends HTMLElement {
    static get observedAttributes() {
        return ['angle_true', 'angle_app'];
    }

    constructor() {
        super();
        this.arrowTrue = null;
        this.arrowApp = null;
    }

    connectedCallback() {
        this.render();
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (oldValue !== newValue && this.arrowTrue && this.arrowApp) {
            this.updateArrows();
        }
    }

    render() {
        this.innerHTML = '';
        const svg = buildCompassRoseSVG();

        // True wind arrow (blue)
        this.arrowTrue = document.createElementNS("http://www.w3.org/2000/svg", "path");
        this.arrowTrue.setAttributeNS(null, "d", "M95 105 L105 45 L115 105 Z");
        this.arrowTrue.setAttributeNS(null, "fill", "blue");
        this.arrowTrue.setAttributeNS(null, "stroke", "gray");
        this.arrowTrue.setAttributeNS(null, "stroke-width", "3");
        svg.appendChild(this.arrowTrue);

        // Apparent wind arrow (light blue)
        this.arrowApp = document.createElementNS("http://www.w3.org/2000/svg", "path");
        this.arrowApp.setAttributeNS(null, "d", "M95 105 L105 45 L115 105 Z");
        this.arrowApp.setAttributeNS(null, "fill", "lightblue");
        this.arrowApp.setAttributeNS(null, "stroke", "gray");
        this.arrowApp.setAttributeNS(null, "stroke-width", "3");
        svg.appendChild(this.arrowApp);

        // Port arc (red)
        const arcRed = document.createElementNS("http://www.w3.org/2000/svg", "path");
        arcRed.setAttributeNS(null, "d", "M35 105 A70 70 00 0 1 105 35");
        arcRed.setAttributeNS(null, "fill", "none");
        arcRed.setAttributeNS(null, "stroke", "red");
        arcRed.setAttributeNS(null, "stroke-width", "5");
        svg.appendChild(arcRed);

        // Starboard arc (green)
        const arcGreen = document.createElementNS("http://www.w3.org/2000/svg", "path");
        arcGreen.setAttributeNS(null, "d", "M35 105 A70 70 00 0 1 105 35");
        arcGreen.setAttributeNS(null, "fill", "none");
        arcGreen.setAttributeNS(null, "stroke", "green");
        arcGreen.setAttributeNS(null, "stroke-width", "5");
        arcGreen.setAttributeNS(null, "transform", "rotate(90 105 105)");
        svg.appendChild(arcGreen);

        this.appendChild(svg);
        this.updateArrows();
    }

    updateArrows() {
        const angleTrue = parseFloat(this.getAttribute('angle_true')) || 0;
        const angleApp = parseFloat(this.getAttribute('angle_app')) || 0;

        if (this.arrowTrue) {
            this.arrowTrue.setAttributeNS(null, "transform", `rotate(${angleTrue} 105 105)`);
        }
        if (this.arrowApp) {
            this.arrowApp.setAttributeNS(null, "transform", `rotate(${angleApp} 105 105)`);
        }
    }

    refreshTheme() {
        this.render();
    }
});


/**
 * Build SVG roll dial (semi-circle) for displaying boat heel angle
 * Scale: -90° (port heel) to +90° (starboard heel)
 */
// Point on the roll dial's arc for a given roll angle and radius.
// The arc's center is the flat top edge's midpoint (centerY = 0); positive
// radius grows downward, so 0° lands at the bottom of the bowl and +/-45°
// land at the raised ends.
function rollDialPointAt(centerX, centerY, radius, value) {
    const arcAngle = 180 - (value + 90);
    const radians = (arcAngle * Math.PI) / 180;
    return {
        x: centerX + radius * Math.cos(radians),
        y: centerY + radius * Math.sin(radians)
    };
}

// Shallow bowl path (flat chord at the top, arc bulging down through the
// bottom) spanning +/-halfSpan degrees, used for both the case and the rim.
function rollDialBowlPath(centerX, centerY, radius, halfSpan) {
    const left = rollDialPointAt(centerX, centerY, radius, -halfSpan);
    const right = rollDialPointAt(centerX, centerY, radius, halfSpan);
    return `M ${left.x} ${left.y} A ${radius} ${radius} 0 0 0 ${right.x} ${right.y} Z`;
}

function buildRollDialSVG() {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttributeNS(null, "width", "240");
    svg.setAttributeNS(null, "height", "110");
    svg.setAttributeNS(null, "style", "display: block; margin: auto;");

    const centerX = 120;
    const centerY = 0;
    const outerRadius = 108;
    const caseRadius = 100;
    const labelRadius = 84;
    const rimSpan = 61;
    const caseSpan = 55;

    // Yellow-green rim (mimics the instrument's fluid-filled tube), drawn
    // first so the black case on top leaves only a thin rim visible below it.
    const rim = document.createElementNS("http://www.w3.org/2000/svg", "path");
    rim.setAttributeNS(null, "d", rollDialBowlPath(centerX, centerY, outerRadius, rimSpan));
    rim.setAttributeNS(null, "fill", "#c3d92a");
    rim.setAttributeNS(null, "stroke", "#c3d92a");
    rim.setAttributeNS(null, "stroke-width", "8");
    rim.setAttributeNS(null, "stroke-linejoin", "round");
    svg.appendChild(rim);

    // Black case
    const caseShape = document.createElementNS("http://www.w3.org/2000/svg", "path");
    caseShape.setAttributeNS(null, "d", rollDialBowlPath(centerX, centerY, caseRadius, caseSpan));
    caseShape.setAttributeNS(null, "fill", "#1c1c1e");
    caseShape.setAttributeNS(null, "stroke", "#1c1c1e");
    caseShape.setAttributeNS(null, "stroke-width", "8");
    caseShape.setAttributeNS(null, "stroke-linejoin", "round");
    svg.appendChild(caseShape);

    // Scale labels (-45, -30, -15, 0, 15, 30, 45), magnitude only since
    // left/right position already conveys port/starboard.
    [-45, -30, -15, 0, 15, 30, 45].forEach((value) => {
        const p = rollDialPointAt(centerX, centerY, labelRadius, value);
        const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
        text.setAttributeNS(null, "x", p.x);
        text.setAttributeNS(null, "y", p.y);
        text.setAttributeNS(null, "text-anchor", "middle");
        text.setAttributeNS(null, "dominant-baseline", "middle");
        text.setAttributeNS(null, "font-size", "12");
        text.setAttributeNS(null, "font-weight", "600");
        text.setAttributeNS(null, "fill", "#ffffff");
        text.textContent = value === 0 ? "0°" : String(Math.abs(value));
        svg.appendChild(text);
    });

    return svg;
}

/**
 * Custom element: my-roll-dial
 * Displays boat roll angle on a shallow arc gauge, styled after a physical
 * boat inclinometer.
 * Scale: -45° (port heel) to +45° (starboard heel)
 * Attribute: angle (in degrees, positive = starboard heel)
 */
customElements.define('my-roll-dial', class extends HTMLElement {
    static get observedAttributes() {
        return ['angle'];
    }

    constructor() {
        super();
        this.marker = null;
    }

    connectedCallback() {
        this.render();
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (oldValue !== newValue && this.marker) {
            this.updateMarker();
        }
    }

    render() {
        this.innerHTML = '';
        const svg = buildRollDialSVG();

        // Marker slides along the rim to the current heel angle instead of
        // pivoting like a needle.
        this.marker = document.createElementNS("http://www.w3.org/2000/svg", "circle");
        this.marker.setAttributeNS(null, "r", "7");
        this.marker.setAttributeNS(null, "fill", "#1c1c1e");
        this.marker.setAttributeNS(null, "stroke", "#ffffff");
        this.marker.setAttributeNS(null, "stroke-width", "2");
        svg.appendChild(this.marker);

        this.appendChild(svg);
        this.updateMarker();
    }

    updateMarker() {
        const rollAngleDeg = parseFloat(this.getAttribute('angle')) || 0;
        const clampedAngle = Math.max(-45, Math.min(45, rollAngleDeg));
        const markerRadius = 104;
        const p = rollDialPointAt(120, 0, markerRadius, clampedAngle);

        if (this.marker) {
            this.marker.setAttributeNS(null, "cx", p.x);
            this.marker.setAttributeNS(null, "cy", p.y);
        }
    }

    refreshTheme() {
        this.render();
    }
});