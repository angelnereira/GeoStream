-- 0004_history.sql — historial opt-in y analítica k-anónima (E6/E7)
-- Referencia normativa: docs/11_DATA_STORAGE.md §2.0, docs/24_HISTORY_ANALYTICS.md

-- Puntos de posición, particionados por rango sobre captured_at. El worker crea
-- las particiones mensuales con antelación (GS_HISTORY_PARTITION_LOOKAHEAD_DAYS).
CREATE TABLE location_points (
    id              uuid NOT NULL,
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    session_id      uuid NOT NULL,
    member_id       uuid NOT NULL,
    subject_id      uuid NOT NULL,
    device_id       uuid NOT NULL,
    sequence        bigint NOT NULL,
    captured_at     timestamptz NOT NULL,
    received_at     timestamptz NOT NULL,
    latitude        double precision NOT NULL,
    longitude       double precision NOT NULL,
    accuracy_m      double precision NOT NULL,
    speed_mps       double precision,
    heading_deg     double precision,
    motion          text,
    is_mock         boolean NOT NULL DEFAULT false,
    h3_cell         bigint NOT NULL,
    PRIMARY KEY (id, captured_at)
) PARTITION BY RANGE (captured_at);

CREATE INDEX location_points_session_idx ON location_points (tenant_id, session_id, member_id, captured_at);
CREATE INDEX location_points_h3_idx      ON location_points (tenant_id, h3_cell, captured_at);
CREATE INDEX location_points_device_idx  ON location_points (device_id, sequence);

CREATE TABLE trips (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    session_id      uuid NOT NULL,
    member_id       uuid NOT NULL,
    subject_id      uuid NOT NULL,
    started_at      timestamptz NOT NULL,
    ended_at        timestamptz NOT NULL,
    duration_s      integer NOT NULL CHECK (duration_s > 0),
    distance_m      double precision NOT NULL CHECK (distance_m >= 0),
    point_count     integer NOT NULL CHECK (point_count > 0),
    stop_count      integer NOT NULL DEFAULT 0,
    mode            text,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX trips_member_idx ON trips (tenant_id, session_id, member_id, started_at DESC);

-- Polilínea simplificada en formato `gsp1` (24 §3): derivado regenerable.
CREATE TABLE trip_tracks (
    trip_id         uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    seq             integer NOT NULL,
    lat_e7          integer NOT NULL,
    lon_e7          integer NOT NULL,
    PRIMARY KEY (trip_id, seq)
);

CREATE TABLE stops (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    trip_id         uuid NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
    member_id       uuid NOT NULL,
    started_at      timestamptz NOT NULL,
    ended_at        timestamptz NOT NULL,
    duration_s      integer NOT NULL,
    latitude        double precision NOT NULL,
    longitude       double precision NOT NULL,
    radius_m        double precision NOT NULL,
    point_count     integer NOT NULL
);
CREATE INDEX stops_trip_idx ON stops (trip_id, started_at);

-- Agregado k-anónimo por celda y hora. NO contiene identificadores de persona
-- (INV-HIS-3): el mínimo de subjects se comprueba al agregar y la celda se
-- suprime entera si no se alcanza.
CREATE TABLE h3_activity_hourly (
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    h3_cell         bigint NOT NULL,
    bucket_start    timestamptz NOT NULL,
    distinct_subjects integer NOT NULL CHECK (distinct_subjects >= 2),
    point_count     integer NOT NULL,
    stop_count      integer NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, h3_cell, bucket_start)
);
