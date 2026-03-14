class TripCard extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
        this.render();
    }

    render() {
        const trip_data = {
            id: this.getAttribute('data-id'),
            description: this.getAttribute('data-description') || '',
            start_date: this.getAttribute('data-start-date'),
            end_date: this.getAttribute('data-end-date'),
            total_distance_nm: parseFloat(this.getAttribute('data-total-distance')) || 0,
            sailing_distance_nm: parseFloat(this.getAttribute('data-sailing-distance')) || 0,
            motoring_distance_nm: parseFloat(this.getAttribute('data-motoring-distance')) || 0,
            total_time_ms: parseInt(this.getAttribute('data-total-time')) || 0,
            sailing_time_ms: parseInt(this.getAttribute('data-sailing-time')) || 0,
            motoring_time_ms: parseInt(this.getAttribute('data-motoring-time')) || 0,
            moored_time_ms: parseInt(this.getAttribute('data-moored-time')) || 0
        };

        const sailing_percent = trip_data.total_distance_nm > 0 
            ? (trip_data.sailing_distance_nm / trip_data.total_distance_nm * 100) 
            : 0;

        this.shadowRoot.innerHTML = `
            <style>
                :host {
                    display: block;
                    background: var(--bg-secondary);
                    border: 1px solid var(--border-color);
                    border-radius: 8px;
                    padding: 20px;
                    box-shadow: 0 2px 5px var(--card-shadow);
                    cursor: pointer;
                    transition: transform 0.2s, box-shadow 0.2s;
                }

                :host(:hover) {
                    transform: translateY(-2px);
                    box-shadow: 0 4px 15px var(--card-shadow);
                }

                .trip-line-1 {
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    gap: 10px;
                    margin-bottom: 15px;
                }

                .trip-header {
                    flex: 1;
                }

                .trip-title {
                    font-size: 24px;
                    font-weight: bold;
                    color: var(--text-bold);
                }

                .trip-sub-title {
                    color: var(--text-secondary);
                    font-size: 14px;
                }

                .trip-title-id {
                    color: var(--text-secondary);
                    font-size: 16px;
                }

                .trip-buttons {
                    display: flex;
                    gap: 5px;
                }

                .trip-line-2, .trip-line-3, .trip-line-x {
                    display: flex;
                    gap: 20px;
                    flex-wrap: wrap;
                    font-size: 13px;
                    margin-bottom: 12px;
                }

                .line-item {
                    flex: 1;
                    min-width: 200px;
                }

                .label {
                    font-weight: 600;
                    color: var(--text-bold);
                }

                .trip-line-4 {
                    width: 100%;
                }

                .progress-bar {
                    width: 100%;
                    height: 8px;
                    background-color: var(--empty-color);
                    border-radius: 4px;
                    overflow: hidden;
                    display: flex;
                }

                .progress-sailing {
                    height: 100%;
                    background: linear-gradient(to right, #ff9900, #4caf50);
                }

                .progress-motoring {
                    height: 100%;
                    background-color: var(--motoring-color);
                }
            </style>

            <div class="trip-line-1">
                <div class="trip-header">
                    <div class="trip-title">
                        ${trip_data.description || 'Trip ' + trip_data.id}
                        <span class="trip-title-id">(ID: ${trip_data.id})</span><br>
                        <span class="trip-sub-title">${this.format_date(trip_data.start_date)} - ${this.format_date(trip_data.end_date)}</span>
                    </div>
                </div>
                <div class="trip-buttons">
                    <button class="nmea-btn" data-action="edit" title="Edit description">✎</button>
                    <button class="nmea-btn" data-action="export" title="Export trip">⬇</button>
                    <button class="nmea-btn" data-action="trim" title="Trim trip">✂</button>
                    <button class="nmea-btn" data-action="delete" title="Delete trip">🗑</button>
                </div>
            </div>
            <div class="trip-line-2">
                <div class="line-item"><span class="label">Start:</span> ${this.format_date_time(trip_data.start_date)}</div>
                <div class="line-item"><span class="label">End:</span> ${this.format_date_time(trip_data.end_date)}</div>
                <div class="line-item"><span class="label">Duration:</span> ${this.format_duration(trip_data.total_time_ms)}</div>
                <div class="line-item"><span class="label">Moored:</span> ${this.format_duration(trip_data.moored_time_ms)}</div>
            </div>

            <div class="trip-line-3">
                <div class="line-item"><span class="label">Total Distance:</span> ${trip_data.total_distance_nm.toFixed(1)} NM</div>
                <div class="line-item"><span class="label">Sailing:</span> ${trip_data.sailing_distance_nm.toFixed(1)} NM</div>
                <div class="line-item"><span class="label">Motoring:</span> ${trip_data.motoring_distance_nm.toFixed(1)} NM</div>
                <div class="line-item"><span class="label">Sailing %:</span> ${sailing_percent.toFixed(1)}%</div>
            </div>

            <div class="trip-line-4">
                <div class="progress-bar">
                    <div class="progress-sailing" style="width: ${sailing_percent.toFixed(1)}%;" title="Sailing: ${trip_data.sailing_distance_nm.toFixed(1)} NM"></div>
                    <div class="progress-motoring" style="width: ${(100 - sailing_percent).toFixed(1)}%;" title="Motoring: ${trip_data.motoring_distance_nm.toFixed(1)} NM"></div>
                </div>
            </div>
        `;

        this.attach_event_listeners(trip_data);
    }

    attach_event_listeners(trip_data) {
        this.shadowRoot.querySelectorAll('.nmea-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const action = btn.getAttribute('data-action');
                this.dispatchEvent(new CustomEvent('trip-action', {
                    detail: { action, trip_id: trip_data.id, description: trip_data.description },
                    bubbles: true,
                    composed: true
                }));
            });
        });

        this.addEventListener('click', (e) => {
            if (!e.target.classList.contains('nmea-btn')) {
                window.location.href = `trip.html?id=${trip_data.id}`;
            }
        });
    }

    format_date(date_str) {
        if (!date_str) return 'Unknown';
        try {
            const date = new Date(date_str);
            return date.toLocaleDateString();
        } catch (e) {
            return date_str;
        }
    }

    format_date_time(date_str) {
        if (!date_str) return 'Unknown';
        try {
            const date = new Date(date_str);
            return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        } catch (e) {
            return date_str;
        }
    }

    format_duration(ms) {
        if (!ms) return '0h 0m';
        const total_minutes = Math.floor(ms / (1000 * 60));
        const hours = Math.floor(total_minutes / 60);
        const minutes = total_minutes % 60;
        if (hours < 24)
            return hours + 'h ' + minutes + 'm';
        else
            return Math.floor(hours / 24) + 'd ' + (hours % 24) + 'h ' + minutes + 'm';
    }
}

customElements.define('trip-card', TripCard);
