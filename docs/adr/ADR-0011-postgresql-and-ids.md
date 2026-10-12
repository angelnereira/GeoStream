# ADR-0011: PostgreSQL con sqlx e IDs públicos prefijados sobre UUIDv7

- **Estado:** Aceptado (baseline v0.2.0)
- **Fecha:** 2026-10-11
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-OPS-004, NFR-SEC-004

## Contexto

Se necesita una base durable con transacciones para membresías y revocaciones, e IDs opacos no enumerables que no sean credenciales.

## Opciones consideradas

1. PostgreSQL + sqlx con migraciones SQL.
2. ORM (SeaORM/Diesel).
3. Base NoSQL.

## Decisión

PostgreSQL ≥ 16 con `sqlx` y migraciones SQL secuenciales embebidas. Claves `uuid` UUIDv7 generadas por la aplicación; representación pública `<prefijo>_<32 hex>`; estados como `text` con `CHECK`. UUIDv7 revela el instante de creación: se acepta porque no es dato sensible.

## Consecuencias

+ SQL explícito y revisable; restricciones en la base; índices con buena localidad.
- Sin abstracción de ORM; más SQL a mano.

## Seguridad, privacidad y operación

Toda consulta filtra por `tenant_id`; usuario de migraciones separado del de aplicación.

## Referencias

`11_DATA_STORAGE.md`, `05_DOMAIN_MODEL.md` §2
