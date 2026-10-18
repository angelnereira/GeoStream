-- 0003_engine_state.sql — estado del motor de trayectoria (E2) y pools (E4)
-- Referencia normativa: docs/11_DATA_STORAGE.md §2.0

-- Las sesiones existentes quedan en modo sin historial: NOT NULL DEFAULT 'OFF'.
ALTER TABLE sessions
    ADD COLUMN history_mode text NOT NULL DEFAULT 'OFF'
    CHECK (history_mode IN ('OFF', 'TRACE', 'TRACE_AND_ANALYTICS'));

-- Ningún subject gana consentimiento por migrar (INV-SHR-5): DEFAULT '{}'.
ALTER TABLE memberships
    ADD COLUMN consent_purposes text[] NOT NULL DEFAULT '{}';
