-- 0005_edge.sql — tags BLE y rastreadores LPWAN (E8)
-- Referencia normativa: docs/11_DATA_STORAGE.md §2.0, docs/25_EDGE_DEVICES.md

CREATE TABLE tags (
    id              uuid PRIMARY KEY,                 -- tag_...
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    owner_subject_id uuid NOT NULL,
    model           text NOT NULL CHECK (char_length(model) BETWEEN 1 AND 100),
    name            text,
    state           text NOT NULL DEFAULT 'ACTIVE'
                        CHECK (state IN ('ACTIVE', 'SEPARATED', 'REMOVED')),
    separated_at    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);
-- INV-EDGE-1: no existe columna para la EIK. La clave vive en el tag y en el
-- dispositivo dueño; el servidor nunca la recibe ni la almacena.
CREATE INDEX tags_owner_idx ON tags (tenant_id, owner_subject_id, state);

CREATE TABLE edge_reports (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    tag_id          uuid NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    hashed_eid      bytea NOT NULL,
    finder_device_id uuid NOT NULL,
    finder_ephemeral_pub bytea NOT NULL,
    payload_salt    bytea NOT NULL,
    ciphertext      bytea NOT NULL,
    time_bucket     timestamptz NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL
);
CREATE INDEX edge_reports_lookup_idx ON edge_reports (tag_id, hashed_eid, time_bucket);
-- El blob es opaco: el servidor no tiene la EIK ni clave de descifrado.

CREATE TABLE edge_lpwan_integrations (
    id              uuid PRIMARY KEY,                 -- lpw_...
    tenant_id       uuid NOT NULL REFERENCES tenants(id),
    kind            text NOT NULL CHECK (kind IN ('LORAWAN', 'NBIOT')),
    decoder         text NOT NULL,
    secret_hash     bytea NOT NULL,
    enabled         boolean NOT NULL DEFAULT true,
    created_at      timestamptz NOT NULL DEFAULT now()
);
