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
        this.arrowTrue = document.createElementNS("http://www.w3.org/2000/svg", "path");
        this.arrowTrue.setAttributeNS(null, "d", "M95 105 L105 45 L115 105 Z");
        this.arrowTrue.setAttributeNS(null, "fill", "blue");
        this.arrowTrue.setAttributeNS(null, "stroke", "gray");
        this.arrowTrue.setAttributeNS(null, "stroke-width", "3");
        svg.appendChild(this.arrowTrue);

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