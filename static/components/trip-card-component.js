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
            moored_time_ms: parseInt(this.getAttribute('data-moored-time')) || 0,
            uuid: this.getAttribute('data-uuid') || ''
        };

        const sailing_percent = trip_data.total_distance_nm > 0 
            ? (trip_data.sailing_distance_nm / trip_data.total_distance_nm * 100) 
            : 0;

        const motoring_percent = trip_data.total_distance_nm > 0 
            ? (trip_data.motoring_distance_nm / trip_data.total_distance_nm * 100) 
            : 0;

        this.shadowRoot.innerHTML = `
            <link rel="stylesheet" href="/shared.css">
            <script src="/js/shared-theme.js"></script>
            <style>
                :host {
                    display: block;
                }

                .trip-line {
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
                    color: var(--text-bold);
                }

                .trip-sub-title {
                    color: var(--text-secondary);
                    font-size: 14px;
                }

                .trip-sub-title-tiny {
                    color: var(--text-secondary);
                    font-size: 10px;
                }

                .trip-buttons {
                    display: flex;
                    gap: 5px;
                }
            </style>

            <div class="app-card">
                <div class="trip-line">
                    <div class="trip-header">
                        <div class="trip-title">
                            ${trip_data.description || 'Trip ' + trip_data.id}
                            <span class="trip-sub-title-tiny">(ID: ${trip_data.id}${trip_data.uuid ? `, UUID: ${trip_data.uuid}` : ''})</span><br>
                            <span class="trip-sub-title">${this.format_date(trip_data.start_date)} - ${this.format_date(trip_data.end_date)}</span>
                        </div>
                    </div>
                    <div class="trip-buttons">
                        <button class="app-btn" data-action="edit" title="Edit description"><app-icon name="pencil"></app-icon></button>
                        <button class="app-btn" data-action="export" title="Export trip"><app-icon name="download"></app-icon></button>
                        <button class="app-btn" data-action="trim" title="Trim trip"><app-icon name="scissors"></app-icon></button>
                        <button class="app-btn" data-action="delete" title="Delete trip"><app-icon name="trash"></app-icon></button>
                    </div>
                </div>


                <div class="trip-line">
                    <div class="card-stats">
                    <div class="card-stat">
                        <div class="card-stat-label">Total</div>
                        <div class="card-stat-value">${trip_data.total_distance_nm.toFixed(1)}<span class="unit-sm"> NM</span></div>
                        <div class="card-stat-details">${this.format_duration(trip_data.total_time_ms)}</div>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="card-stat">
                        <div class="card-stat-label">Sailing Distance</div>
                        <div class="card-stat-value">${trip_data.sailing_distance_nm.toFixed(1)}<span class="unit-sm"> NM</span></div>
                        <div class="card-stat-details"><span class="sailing-percentage">${sailing_percent.toFixed(1)}%</span></div>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="card-stat">
                        <div class="card-stat-label">Motoring Distance</div>
                        <div class="card-stat-value">${trip_data.motoring_distance_nm.toFixed(1)}<span class="unit-sm"> NM</span></div>
                        <div class="card-stat-details"><span class="motoring-percentage">${motoring_percent.toFixed(1)}%</span></div>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="card-stat">
                        <div class="card-stat-label">Start</div>
                        <div class="card-stat-value">${this.format_date(trip_data.start_date)}</div>
                        <div class="card-stat-details">${this.format_time(trip_data.start_date)}</div>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="card-stat">
                        <div class="card-stat-label">End</div>
                        <div class="card-stat-value">${this.format_date(trip_data.end_date)}</div>
                        <div class="card-stat-details">${this.format_time(trip_data.end_date)}</div>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="card-stat">
                        <div class="card-stat-label">Moored</div>
                        <div class="card-stat-value">${this.format_duration(trip_data.moored_time_ms)}</div>
                    </div>
                    </div>
                </div>

                <div class="trip-line">
                    <div style="width: 100%">
                        <voyage-bar
                            sailing-distance="${trip_data.sailing_distance_nm.toFixed(1)}"
                            motoring-distance="${trip_data.motoring_distance_nm.toFixed(1)}"
                            total-distance="${trip_data.total_distance_nm.toFixed(1)}">
                        </voyage-bar>
                    </div>
                </div>

            </div>
        `;

        this.attach_event_listeners(trip_data);
    }

    attach_event_listeners(trip_data) {
        this.shadowRoot.querySelectorAll('.app-btn').forEach(btn => {
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
            if (!e.target.classList.contains('app-btn')) {
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

    format_time(date_str) {
        if (!date_str) return 'Unknown';
        try {
            const date = new Date(date_str);
            return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
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
