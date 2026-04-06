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
function buildRollDialSVG() {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttributeNS(null, "width", "210");
    svg.setAttributeNS(null, "height", "105");
    svg.setAttributeNS(null, "style", "display: block; margin: auto;");

    // Get theme-aware colors
    const bgColor = getComputedStyle(document.documentElement).getPropertyValue('--bg-secondary') || '#EEEEFF';
    const tickColor = getComputedStyle(document.documentElement).getPropertyValue('--text-secondary') || '#000000';

    // Circle background
    const circle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    circle.setAttributeNS(null, "cx", "50%");
    circle.setAttributeNS(null, "cy", "100%");
    circle.setAttributeNS(null, "r", "90");
    circle.setAttributeNS(null, "stroke", "var(--border-color)");
    circle.setAttributeNS(null, "stroke-width", "3");
    circle.setAttributeNS(null, "fill", bgColor.trim());
    svg.appendChild(circle);

    // Tick marks and labels (-90, -60, -30, 0, 30, 60, 90)
    const angles = [
        { value: -90, label: "Port" },
        { value: -75, label: "" },
        { value: -60, label: "" },
        { value: -45, label: "" },
        { value: -30, label: "" },
        { value: -15, label: "" },
        { value: -10, label: "" },
        { value: -5, label: "" },
        { value: 0, label: "0°" },
        { value: 5, label: "" },
        { value: 10, label: "" },
        { value: 15, label: "" },
        { value: 30, label: "" },
        { value: 45, label: "" },
        { value: 60, label: "" },
        { value: 75, label: "" },
        { value: 90, label: "Starboard" }
    ];

    angles.forEach(({ value, label }) => {
        // Convert roll angle (-90 to +90) to arc angle (180 to 0 degrees)
        const arcAngle = 180 - (value + 90);
        const radians = (arcAngle * Math.PI) / 180;
        
        // Calculate tick position on the arc
        const radius = 90;
        const centerX = 105;
        const centerY = 105;
        const x = centerX + radius * Math.cos(radians);
        const y = centerY - radius * Math.sin(radians);
        
        // Calculate inner tick position
        const tickLength = value % 30 === 0 ? 15 : 8;
        const innerRadius = radius - tickLength;
        const innerX = centerX + innerRadius * Math.cos(radians);
        const innerY = centerY - innerRadius * Math.sin(radians);

        // Draw tick mark
        const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
        line.setAttributeNS(null, "x1", x);
        line.setAttributeNS(null, "y1", y);
        line.setAttributeNS(null, "x2", innerX);
        line.setAttributeNS(null, "y2", innerY);
        line.setAttributeNS(null, "stroke", tickColor.trim() || "black");
        line.setAttributeNS(null, "stroke-width", value % 30 === 0 ? "2" : "1");
        svg.appendChild(line);

        // Draw label for major ticks
        if (label) {
            const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
            const labelRadius = radius - 30;
            const labelX = centerX + labelRadius * Math.cos(radians);
            const labelY = centerY - labelRadius * Math.sin(radians);
            
            text.setAttributeNS(null, "x", labelX);
            text.setAttributeNS(null, "y", labelY);
            text.setAttributeNS(null, "text-anchor", "middle");
            text.setAttributeNS(null, "dominant-baseline", "middle");
            text.setAttributeNS(null, "font-size", "12");
            text.setAttributeNS(null, "fill", tickColor.trim() || "black");
            text.textContent = label;
            svg.appendChild(text);
        }
    });

    // Center circle
    const centerDot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    centerDot.setAttributeNS(null, "cx", "105");
    centerDot.setAttributeNS(null, "cy", "105");
    centerDot.setAttributeNS(null, "r", "5");
    centerDot.setAttributeNS(null, "fill", "var(--text-primary)");
    svg.appendChild(centerDot);

    return svg;
}

/**
 * Custom element: my-roll-dial
 * Displays boat roll angle on a semi-circular gauge
 * Scale: -90° (port heel) to +90° (starboard heel)
 * Attribute: angle (in degrees, positive = starboard heel)
 */
customElements.define('my-roll-dial', class extends HTMLElement {
    static get observedAttributes() {
        return ['angle'];
    }

    constructor() {
        super();
        this.needle = null;
    }

    connectedCallback() {
        this.render();
    }

    attributeChangedCallback(name, oldValue, newValue) {
        if (oldValue !== newValue && this.needle) {
            this.updateNeedle();
        }
    }

    render() {
        this.innerHTML = '';
        const svg = buildRollDialSVG();

        // Needle (red line from center pointing to current roll angle)
        this.needle = document.createElementNS("http://www.w3.org/2000/svg", "line");
        this.needle.setAttributeNS(null, "x1", "105");
        this.needle.setAttributeNS(null, "y1", "105");
        this.needle.setAttributeNS(null, "x2", "105");
        this.needle.setAttributeNS(null, "y2", "35");
        this.needle.setAttributeNS(null, "stroke", "#e74c3c");
        this.needle.setAttributeNS(null, "stroke-width", "4");
        this.needle.setAttributeNS(null, "stroke-linecap", "round");
        this.needle.setAttributeNS(null, "transform-origin", "105 105");
        svg.appendChild(this.needle);

        this.appendChild(svg);
        this.updateNeedle();
    }

    updateNeedle() {
        const rollAngleDeg = parseFloat(this.getAttribute('angle')) || 0;
        
        // Clamp to -90 to +90 range
        const clampedAngle = Math.max(-90, Math.min(90, rollAngleDeg));
        
        // Convert roll angle (-90 to +90) to rotation (-180 to 0 degrees)
        // At -90 (port), needle points left (-180°)
        // At 0 (upright), needle points up (-90°)
        // At +90 (starboard), needle points right (0°)
        const rotationAngle = -90 - clampedAngle;

        if (this.needle) {
            this.needle.setAttributeNS(null, "transform", `rotate(${rotationAngle} 105 105)`);
        }
    }

    refreshTheme() {
        this.render();
    }
});