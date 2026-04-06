class VoyageBar extends HTMLElement {
    static get observedAttributes() {
        return ['sailing-distance', 'motoring-distance', 'total-distance'];
    }

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
        this.render();
    }

    attributeChangedCallback() {
        this.render();
    }

    render() {
        const sailing_distance = parseFloat(this.getAttribute('sailing-distance')) || 0;
        const motoring_distance = parseFloat(this.getAttribute('motoring-distance')) || 0;
        const total_distance = parseFloat(this.getAttribute('total-distance')) || (sailing_distance + motoring_distance);

        const sailing_percent = total_distance > 0 ? (sailing_distance / total_distance * 100) : 0;
        const motoring_percent = total_distance > 0 ? (motoring_distance / total_distance * 100) : 0;

        this.shadowRoot.innerHTML = `
            <link rel="stylesheet" href="/shared.css">
            <div class="voyage-bar-wrap">
                <div class="voyage-bar">
                    <div class="vb-sail" style="width: ${sailing_percent.toFixed(1)}%"></div>
                    <div class="vb-motor" style="width: ${motoring_percent.toFixed(1)}%"></div>
                </div>
                <div class="voyage-bar-legend">
                    <div class="vbl-item">
                        <div class="vbl-dot sail"></div>
                        <span><span class="vbl-val">${sailing_distance.toFixed(1)} NM</span> sailing</span>
                    </div>
                    <div class="vbl-item" style="margin-left: auto;">
                        <div class="vbl-dot motor"></div>
                        <span><span class="vbl-val">${motoring_distance.toFixed(1)} NM</span> motoring</span>
                    </div>
                </div>
            </div>
        `;
    }
}

customElements.define('voyage-bar', VoyageBar);
