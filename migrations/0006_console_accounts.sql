-- 0006_console_accounts.sql — cuentas, códigos, sesiones y membresías de la consola (F15)
-- Referencia normativa: docs/11_DATA_STORAGE.md §2.0.1, ADR-0033 §3, ADR-0036.
-- Aditiva: ninguna tabla existente cambia.

-- Cuenta de consola. Sin contraseña (ADR-0036). El correo es un dato personal (docs/10 §6.2).
CREATE TABLE console_accounts (
    id                uuid PRIMARY KEY,                 -- acc_...
    email_normalized  text NOT NULL
                          CHECK (char_length(email_normalized) BETWEEN 3 AND 254
                                 AND email_normalized = lower(email_normalized)),
    email_hash        bytea NOT NULL CHECK (octet_length(email_hash) = 32),
    status            text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'DISABLED')),
    terms_accepted_at timestamptz NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX console_accounts_email_uq ON console_accounts (email_normalized);
CREATE UNIQUE INDEX console_accounts_email_hash_uq ON console_accounts (email_hash);

-- Código de un solo uso. Solo se guarda el HMAC del código, nunca el código.
-- Como mucho una fila viva por correo (índice parcial): pedir otro código la reemplaza.
CREATE TABLE console_otp_codes (
    id           uuid PRIMARY KEY,
    email_hash   bytea NOT NULL CHECK (octet_length(email_hash) = 32),
    code_hmac    bytea NOT NULL CHECK (octet_length(code_hmac) = 32),
    expires_at   timestamptz NOT NULL,
    attempts     integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    consumed_at  timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX console_otp_live_uq ON console_otp_codes (email_hash) WHERE consumed_at IS NULL;
CREATE INDEX console_otp_expiry_idx ON console_otp_codes (expires_at);

-- Sesión de consola. Solo se guarda el SHA-256 del token opaco.
CREATE TABLE console_sessions (
    id             uuid PRIMARY KEY,
    account_id     uuid NOT NULL REFERENCES console_accounts (id) ON DELETE CASCADE,
    token_sha256   bytea NOT NULL CHECK (octet_length(token_sha256) = 32),
    expires_at     timestamptz NOT NULL,
    revoked_at     timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX console_sessions_token_uq ON console_sessions (token_sha256);
CREATE INDEX console_sessions_account_idx ON console_sessions (account_id);

-- Reservada para «Entrar con Gravital» (T-F15-20): enlaza una cuenta con un emisor externo.
CREATE TABLE console_identities (
    account_id  uuid NOT NULL REFERENCES console_accounts (id) ON DELETE CASCADE,
    issuer      text NOT NULL CHECK (char_length(issuer) BETWEEN 1 AND 200),
    subject     text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 200),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, subject)
);
CREATE INDEX console_identities_account_idx ON console_identities (account_id);

-- Quién puede administrar qué organización (tenant). Todo acceso a un tenant desde la consola se filtra por aquí.
CREATE TABLE tenant_memberships (
    account_id  uuid NOT NULL REFERENCES console_accounts (id) ON DELETE CASCADE,
    tenant_id   uuid NOT NULL REFERENCES tenants (id),
    role        text NOT NULL CHECK (role IN ('OWNER', 'ADMIN', 'VIEWER')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, tenant_id)
);
CREATE INDEX tenant_memberships_tenant_idx ON tenant_memberships (tenant_id);

-- Tope diario global de correos enviados (ADR-0036 §2): evita convertir el registro en un coste por correo.
CREATE TABLE console_email_counters (
    day   date PRIMARY KEY,                              -- día UTC
    sent  integer NOT NULL DEFAULT 0 CHECK (sent >= 0)
);
