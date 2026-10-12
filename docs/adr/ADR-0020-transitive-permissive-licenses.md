# ADR-0020: Excepciones de licencias permisivas para dependencias transitivas

- **Estado:** Aceptado
- **Fecha:** 2026-10-12
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** NFR-SEC-006 (auditoría de dependencias y licencias)

## Contexto

`19_ENGINEERING_STANDARDS.md` §6.1 fija las licencias permitidas: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0; MPL-2.0 permitida como dependencia sin modificar. Cualquier otra requiere un ADR.

Al fijar las dependencias aprobadas en `Cargo.lock` (T-F0-02), `cargo deny check licenses` identificó cuatro paquetes transitivos indispensables con licencias permisivas que no estaban en la lista cerrada:
- `xxhash-rust` (BSL-1.0), requerido por `redis`.
- `webpki-root-certs` (CDLA-Permissive-2.0), requerido por `reqwest` con TLS.
- `webpki-roots` (CDLA-Permissive-2.0), requerido por `sqlx` con TLS.
- `borrow-or-share` (MIT-0), requerido por `jsonschema` en pruebas.

El propietario del proyecto ha definido que el desarrollo debe realizarse de forma que permita utilizar las tecnologías libremente y comercializar, distribuir o vender GeoStream sin restricciones restrictivas (copyleft).

## Opciones consideradas

1. Excepciones por paquete en `deny.toml` (`[[licenses.exceptions]]`) documentadas en este ADR.
2. Añadir BSL-1.0, CDLA-Permissive-2.0 y MIT-0 a la lista global de licencias permitidas.
3. Evitar las dependencias (inviable sin retirar `redis`, `sqlx` o `reqwest`).

## Decisión

Adoptar la Opción 1: mantener la lista general de licencias aprobadas en `deny.toml` y `19` §6.1, y autorizar excepciones por paquete para `xxhash-rust` (BSL-1.0), `webpki-root-certs` (CDLA-Permissive-2.0), `webpki-roots` (CDLA-Permissive-2.0) y `borrow-or-share` (MIT-0).
Estas tres licencias son permisivas (sin copyleft virulento) y compatibles con la libre utilización, distribución y venta de GeoStream.

## Consecuencias

+ Desbloquea T-F0-02 y permite que `cargo deny check` pase limpiamente en local y en CI.
+ Mantiene control estricto: cualquier otro paquete con licencia no listada seguirá fallando y requerirá revisión explícita.
+ Garantiza la compatibilidad comercial y de distribución del proyecto.

## Referencias

- `DR-20261012-01`
- `docs/19_ENGINEERING_STANDARDS.md` §6.1
- `deny.toml`
