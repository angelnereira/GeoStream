# 11 — Datos y almacenamiento

**ID:** GS-DAT-001 · **Versión:** 0.2.0 · **Decisiones:** ADR-0005, ADR-0010, ADR-0011

## 1. Reparto de responsabilidades

| Almacén | Contiene | No contiene |
|---|---|---|
| PostgreSQL ≥ 16 | Verdad durable: tenants, aplicaciones, keys, subjects, dispositivos, sesiones, membresías, invitaciones, auditoría, idempotencia, webhooks, outbox, geocercas | Coordenadas de personas (salvo historial opt-in F13) |
| Redis/Valkey ≥ 7.2 (o memoria en nodo único) | Snapshots, secuencias, presencia, tickets, versiones de membresía, rate limits, bus | Nada que no pueda perderse |

## 2. Esquema PostgreSQL inicial (normativo)

La migración `migrations/0001_init.sql` DEBE implementar exactamente este esquema (nombres, tipos, restricciones e índices). Cambios posteriores: nuevas migraciones (§4).

Convenciones: claves primarias `uuid` (UUIDv7 generado por la aplicación, nunca por la base de datos); `timestamptz` siempre; estados como `text` con `CHECK` (no tipos `ENUM` de PostgreSQL, para facilitar migraciones); toda tabla de negocio lleva `tenant_id`.

```sql
-- 0001_init.sql — GeoStream esquema inicial (F2/F3)

CREATE TABLE tenants (
    id              uuid PRIMARY KEY,
    name            text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    status          text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'DISABLED')),
    policy          jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE applications (
    id                      uuid PRIMARY KEY,
    tenant_id               uuid NOT NULL REFERENCES tenants (id),
    name                    text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    status                  text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'DISABLED')),
    allowed_device_scopes   text[] NOT NULL DEFAULT ARRAY['sessions:create', 'invitations:redeem', 'stream:connect', 'sharing:write', 'ingest:write'],
    allowed_origins         text[] NOT NULL DEFAULT '{}',
    created_at              timestamptz NOT NULL DEFAULT now(),
    updated_at              timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX applications_tenant_idx ON applications (tenant_id);

CREATE TABLE api_keys (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants (id),
    application_id  uuid NOT NULL REFERENCES applications (id),
    secret_sha256   bytea NOT NULL CHECK (octet_length(secret_sha256) = 32),
    scopes          text[] NOT NULL,
    label           text CHECK (label IS NULL OR char_length(label) <= 100),
    status          text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'REVOKED')),
    expires_at      timestamptz,
    last_used_at    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz,
    CHECK ((status = 'REVOKED') = (revoked_at IS NOT NULL))
);
CREATE INDEX api_keys_application_idx ON api_keys (application_id);

CREATE TABLE subjects (
    id                    uuid PRIMARY KEY,
    tenant_id             uuid NOT NULL REFERENCES tenants (id),
    external_subject_id   text CHECK (external_subject_id IS NULL OR external_subject_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    status                text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'ERASED')),
    token_epoch           integer NOT NULL DEFAULT 0 CHECK (token_epoch >= 0),
    created_at            timestamptz NOT NULL DEFAULT now(),
    updated_at            timestamptz NOT NULL DEFAULT now(),
    erased_at             timestamptz,
    CHECK ((status = 'ERASED') = (external_subject_id IS NULL)),
    CHECK ((status = 'ERASED') = (erased_at IS NOT NULL))
);
CREATE UNIQUE INDEX subjects_tenant_external_uq ON subjects (tenant_id, external_subject_id) WHERE status = 'ACTIVE';

CREATE TABLE devices (
    id               uuid PRIMARY KEY,
    tenant_id        uuid NOT NULL REFERENCES tenants (id),
    subject_id       uuid NOT NULL REFERENCES subjects (id),
    application_id   uuid NOT NULL REFERENCES applications (id),
    installation_id  uuid NOT NULL,
    platform         text NOT NULL CHECK (platform IN ('ANDROID', 'IOS', 'WEB', 'SERVER', 'OTHER')),
    app_version      text CHECK (app_version IS NULL OR char_length(app_version) <= 32),
    sdk_version      text CHECK (sdk_version IS NULL OR char_length(sdk_version) <= 32),
    status           text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'INACTIVE')),
    created_at       timestamptz NOT NULL DEFAULT now(),
    last_seen_at     timestamptz NOT NULL DEFAULT now(),
    deactivated_at   timestamptz,
    CHECK ((status = 'INACTIVE') = (deactivated_at IS NOT NULL))
);
CREATE UNIQUE INDEX devices_identity_uq ON devices (application_id, installation_id, subject_id) WHERE status = 'ACTIVE';
CREATE INDEX devices_subject_active_idx ON devices (subject_id, last_seen_at) WHERE status = 'ACTIVE';

CREATE TABLE sessions (
    id                     uuid PRIMARY KEY,
    tenant_id              uuid NOT NULL REFERENCES tenants (id),
    mode                   text NOT NULL CHECK (mode IN ('ONE_TO_ONE', 'ONE_TO_MANY', 'MANY_TO_MANY')),
    status                 text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'CLOSED', 'EXPIRED')),
    owner_subject_id       uuid REFERENCES subjects (id),
    created_by_kind        text NOT NULL CHECK (created_by_kind IN ('DEVICE', 'APPLICATION')),
    created_by_id          uuid NOT NULL,
    expires_at             timestamptz NOT NULL,
    max_members            integer NOT NULL CHECK (max_members >= 2),
    snapshot_ttl_seconds   integer NOT NULL CHECK (snapshot_ttl_seconds >= 0),
    stale_after_seconds    integer NOT NULL CHECK (stale_after_seconds BETWEEN 5 AND 3600),
    metadata               jsonb NOT NULL DEFAULT '{}'::jsonb,
    closed_reason          text CHECK (closed_reason IN ('CLOSED_BY_MEMBER', 'CLOSED_BY_APPLICATION', 'OWNER_LEFT', 'ADMIN')),
    closed_at              timestamptz,
    version                bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now(),
    CHECK (mode <> 'ONE_TO_ONE' OR max_members = 2),
    CHECK ((status = 'ACTIVE') = (closed_at IS NULL)),
    CHECK (status <> 'CLOSED' OR closed_reason IS NOT NULL)
);
CREATE INDEX sessions_tenant_created_idx ON sessions (tenant_id, created_at DESC, id DESC);
CREATE INDEX sessions_active_expiry_idx ON sessions (expires_at) WHERE status = 'ACTIVE';
CREATE INDEX sessions_closed_at_idx ON sessions (closed_at) WHERE status <> 'ACTIVE';

CREATE TABLE memberships (
    id                    uuid PRIMARY KEY,
    tenant_id             uuid NOT NULL REFERENCES tenants (id),
    session_id            uuid NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    subject_id            uuid NOT NULL REFERENCES subjects (id),
    role                  text NOT NULL CHECK (role IN ('OWNER', 'MEMBER')),
    can_publish           boolean NOT NULL,
    can_read              boolean NOT NULL,
    can_invite            boolean NOT NULL,
    can_manage            boolean NOT NULL,
    status                text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'LEFT', 'REVOKED')),
    sharing               boolean NOT NULL DEFAULT false,
    publisher_device_id   uuid REFERENCES devices (id),
    sharing_changed_at    timestamptz,
    joined_via            text NOT NULL CHECK (joined_via IN ('CREATOR', 'DIRECT_ADD', 'INVITATION')),
    invitation_id         uuid,
    joined_at             timestamptz NOT NULL DEFAULT now(),
    ended_at              timestamptz,
    version               bigint NOT NULL DEFAULT 1 CHECK (version >= 1),
    CHECK (sharing = (publisher_device_id IS NOT NULL)),
    CHECK (NOT sharing OR (status = 'ACTIVE' AND can_publish)),
    CHECK (role <> 'OWNER' OR (can_publish AND can_read AND can_invite AND can_manage)),
    CHECK ((status = 'ACTIVE') = (ended_at IS NULL)),
    CHECK ((joined_via = 'INVITATION') = (invitation_id IS NOT NULL))
);
CREATE UNIQUE INDEX memberships_active_uq ON memberships (session_id, subject_id) WHERE status = 'ACTIVE';
CREATE UNIQUE INDEX memberships_single_owner_uq ON memberships (session_id) WHERE status = 'ACTIVE' AND role = 'OWNER';
CREATE INDEX memberships_session_order_idx ON memberships (session_id, joined_at, id);
CREATE INDEX memberships_subject_active_idx ON memberships (subject_id) WHERE status = 'ACTIVE';
CREATE INDEX memberships_publisher_idx ON memberships (publisher_device_id) WHERE sharing;

CREATE TABLE invitations (
    id                     uuid PRIMARY KEY,
    tenant_id              uuid NOT NULL REFERENCES tenants (id),
    session_id             uuid NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    code_sha256            bytea NOT NULL CHECK (octet_length(code_sha256) = 32),
    grant_publish          boolean NOT NULL,
    grant_read             boolean NOT NULL,
    grant_invite           boolean NOT NULL,
    grant_manage           boolean NOT NULL,
    max_uses               integer NOT NULL CHECK (max_uses >= 1),
    uses                   integer NOT NULL DEFAULT 0 CHECK (uses >= 0 AND uses <= max_uses),
    expires_at             timestamptz NOT NULL,
    created_by_kind        text NOT NULL CHECK (created_by_kind IN ('DEVICE', 'APPLICATION')),
    created_by_id          uuid NOT NULL,
    created_by_member_id   uuid,
    created_at             timestamptz NOT NULL DEFAULT now(),
    revoked_at             timestamptz
);
CREATE UNIQUE INDEX invitations_code_uq ON invitations (code_sha256);
CREATE INDEX invitations_session_idx ON invitations (session_id, created_at DESC, id DESC);
CREATE INDEX invitations_expiry_idx ON invitations (expires_at);

CREATE TABLE audit_events (
    id            uuid PRIMARY KEY,
    tenant_id     uuid NOT NULL,
    occurred_at   timestamptz NOT NULL DEFAULT now(),
    actor_kind    text NOT NULL CHECK (actor_kind IN ('DEVICE', 'APPLICATION', 'OPERATOR', 'SYSTEM')),
    actor_id      uuid,
    subject_id    uuid,
    action        text NOT NULL CHECK (char_length(action) BETWEEN 1 AND 64),
    target_kind   text,
    target_id     uuid,
    outcome       text NOT NULL CHECK (outcome IN ('SUCCESS', 'DENIED', 'FAILED')),
    request_id    uuid,
    details       jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX audit_events_tenant_time_idx ON audit_events (tenant_id, occurred_at DESC);
CREATE INDEX audit_events_subject_idx ON audit_events (subject_id) WHERE subject_id IS NOT NULL;
CREATE INDEX audit_events_time_idx ON audit_events (occurred_at);

CREATE TABLE idempotency_keys (
    tenant_id           uuid NOT NULL,
    principal_id        uuid NOT NULL,
    idempotency_key     text NOT NULL CHECK (char_length(idempotency_key) BETWEEN 1 AND 255),
    request_sha256      bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    state               text NOT NULL CHECK (state IN ('IN_PROGRESS', 'COMPLETED')),
    response_status     smallint,
    response_body_enc   bytea,
    created_at          timestamptz NOT NULL DEFAULT now(),
    expires_at          timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, principal_id, idempotency_key),
    CHECK ((state = 'COMPLETED') = (response_status IS NOT NULL))
);
CREATE INDEX idempotency_keys_expiry_idx ON idempotency_keys (expires_at);

CREATE TABLE webhook_endpoints (
    id                     uuid PRIMARY KEY,
    tenant_id              uuid NOT NULL REFERENCES tenants (id),
    url                    text NOT NULL CHECK (char_length(url) <= 2048),
    event_types            text[] NOT NULL CHECK (cardinality(event_types) >= 1),
    description            text CHECK (description IS NULL OR char_length(description) <= 200),
    secret_enc             bytea NOT NULL,
    status                 text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'DISABLED')),
    disabled_reason        text,
    consecutive_failures   integer NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    failing_since          timestamptz,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX webhook_endpoints_tenant_idx ON webhook_endpoints (tenant_id);

CREATE TABLE webhook_deliveries (
    id                 uuid PRIMARY KEY,
    event_id           uuid NOT NULL,
    tenant_id          uuid NOT NULL,
    endpoint_id        uuid NOT NULL REFERENCES webhook_endpoints (id) ON DELETE CASCADE,
    event_type         text NOT NULL,
    payload            jsonb NOT NULL,
    status             text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'DELIVERED', 'FAILED', 'CANCELLED')),
    attempts           integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at    timestamptz NOT NULL DEFAULT now(),
    last_status_code   smallint,
    last_error         text CHECK (last_error IS NULL OR char_length(last_error) <= 500),
    created_at         timestamptz NOT NULL DEFAULT now(),
    finished_at        timestamptz,
    UNIQUE (event_id, endpoint_id)
);
CREATE INDEX webhook_deliveries_due_idx ON webhook_deliveries (next_attempt_at) WHERE status = 'PENDING';
CREATE INDEX webhook_deliveries_finished_idx ON webhook_deliveries (finished_at) WHERE status <> 'PENDING';
```

Notas normativas:
- El outbox de webhooks es `webhook_deliveries`: el caso de uso inserta una fila por endpoint `ACTIVE` del tenant cuyo `event_types` contiene el tipo, **en la misma transacción** que el cambio. En F2–F7 (antes de webhooks) la tabla existe pero no recibe filas porque no hay endpoints.
- `audit_events` no tiene claves foráneas a propósito: debe sobrevivir a la purga de sesiones.
- La migración de geocercas (F11) añadirá `0003_geofences.sql`:

```sql
-- 0003_geofences.sql (F11)
CREATE TABLE geofences (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants (id),
    session_id      uuid REFERENCES sessions (id) ON DELETE CASCADE,
    name            text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    shape           jsonb NOT NULL,
    metadata        jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_by_kind text NOT NULL CHECK (created_by_kind IN ('DEVICE', 'APPLICATION')),
    created_by_id   uuid NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX geofences_tenant_idx ON geofences (tenant_id);
CREATE INDEX geofences_session_idx ON geofences (session_id) WHERE session_id IS NOT NULL;
```

`0002_*` queda libre para ajustes que surjan en F3–F10 (por ejemplo índices detectados en benchmarks).

### 2.1 Patrones transaccionales obligatorios
- **Alta de miembro / canje**: `SELECT … FROM sessions WHERE id=$1 AND tenant_id=$2 FOR UPDATE` → contar activos → insertar membresía → (canje) `UPDATE invitations SET uses = uses + 1 WHERE id=$1 AND uses < max_uses` comprobando 1 fila afectada → outbox → auditoría → commit → efectos efímeros (`mver++`, bus).
- **Cambio de compartición / revocación / cierre**: transacción PG → commit → borrar snapshot(s) → `mver++` → publicar en bus → responder. Si falla un efecto efímero tras el commit: registrar error, incrementar `gs_ephemeral_effect_failures_total` y **responder igualmente con éxito**; la revalidación por TTL garantiza la convergencia (NFR-SEC-002, peor caso) y el snapshot huérfano expira por su TTL. Excepción: el borrado de snapshot de NFR-PRIV-002 se reintenta hasta 3 veces antes de responder.
- **Aislamiento**: toda consulta incluye `tenant_id = $tenant` derivado del principal, incluso si la clave primaria ya identifica la fila.
- **Nivel de aislamiento**: `READ COMMITTED` con bloqueos explícitos (`FOR UPDATE`) donde se indica; sin `SERIALIZABLE` por defecto.

## 3. Keyspace del almacén efímero

`{p}` = `GS_REDIS_KEY_PREFIX` (default `gs`). Todas las claves tienen TTL. Valores binarios en Protobuf cuando se indica.

| Clave | Tipo | Valor | TTL | Escribe | Lee |
|---|---|---|---|---|---|
| `{p}:snap:{session}:{member}` | string | `LocationUpdate` (Protobuf, `delivery=SNAPSHOT` al leerse) | `snapshot_ttl_seconds` | stream | stream, api |
| `{p}:seq:{session}:{device}` | string | última `sequence` aceptada (decimal) | 7 días, renovado | stream, api (ingesta) | stream, api |
| `{p}:devseq:{device}` | string | mayor `sequence` aceptada del dispositivo | 30 días, renovado | stream, api | stream (hello) |
| `{p}:presence:{device}` | hash | `connection_id`, `instance_id`, `connected_at` | `GS_PRESENCE_TTL_S`, renovado | stream | stream, api |
| `{p}:mver:{session}` | string | versión de membresías (entero) | vida de la sesión + 1 día | api, worker, stream | stream |
| `{p}:ticket:{sha256}` | string | `TicketGrant` (JSON compacto: `principal_kind`, `tenant_id`, `application_id`, `subject_id`/`device_id`/`api_key_id`, `scopes`, `token_epoch` y `auth_expires_at`, `06` §5) | `GS_TICKET_TTL_S` | api | stream (get-and-delete) |
| `{p}:rl:{scope}:{key}:{window}` | string | contador | 120 s | api, stream | api, stream |
| `{p}:geo:{geofence}:{member}` (F11) | string | estado de histéresis | vida de la sesión | stream | stream |

Operaciones atómicas requeridas (scripts Lua versionados en `crates/gs-ephemeral/lua/`, cargados con `SCRIPT LOAD` y ejecutados con `EVALSHA`, con reintento si el script no está cargado):
- `seq_cas.lua`: compare-and-set de secuencia (`09` §4).
- `devseq_max.lua`: `SET` solo si mayor.
- `presence_release.lua`: borrar presencia solo si `connection_id` coincide.
- Ticket: `GETDEL` nativo (Redis ≥ 6.2 / Valkey).

Configuración obligatoria del servidor Redis/Valkey en producción: sin persistencia (`save ""`, `appendonly no`), `maxmemory-policy noeviction`, autenticación, red privada. Ver `16_OPERATIONS.md` §2.

## 4. Migraciones

- Herramienta: `sqlx migrate` embebida en el binario (`geostream migrate`), archivos `migrations/NNNN_descripcion.sql`.
- Solo hacia delante; nunca editar una migración publicada.
- Cambios incompatibles en dos fases (*expand/contract*): (1) añadir columna/tabla nueva compatible y desplegar código que escriba ambas; (2) en un release posterior, migrar datos y retirar lo antiguo.
- Toda migración se prueba: aplicar sobre vacío, aplicar sobre la base del release anterior con datos sintéticos, y ejecutar la suite de integración.
- `geostream serve` comprueba al arrancar que la base está en la versión esperada y falla si no (no migra automáticamente salvo `GS_AUTO_MIGRATE=true`, permitido solo en `GS_ENV=dev`).

## 5. Jobs del rol worker

| Job | Intervalo | Qué hace | Fase |
|---|---|---|---|
| Expiración de sesiones | `GS_WORKER_EXPIRY_INTERVAL_S` | `04` §7.5 | F3 |
| Entrega de webhooks | continuo (sondeo `GS_WORKER_WEBHOOK_POLL_MS`) | `07` §10.4 | F8 |
| Retención | diario (`GS_WORKER_RETENTION_HOUR_UTC`) | `10` §7.1 | F10 |
| Limpieza de idempotencia | cada hora | borrar `expires_at < now()` | F3 |

Todos los jobs: lotes de `GS_WORKER_BATCH_SIZE`, `FOR UPDATE SKIP LOCKED`, seguros con varias instancias, con métricas de duración y filas procesadas.

## 6. Datos de prueba
- Nunca coordenadas reales de personas. Usar puntos públicos (centros de ciudad) o trayectorias sintéticas generadas (`gs-testkit::synthetic`).
- Los fixtures de base de datos se crean con funciones del testkit, no con SQL copiado.
