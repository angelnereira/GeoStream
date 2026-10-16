# ADR-0029: Red de búsqueda GeoStream Beacon: criptografía, trama y alineación con DULT

- **Estado:** Aceptado (parámetros finales de trama BLE sujetos a confirmación en spike T-E8-01 y revisión criptográfica externa en T-E8-02)
- **Fecha:** 2026-10-15
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-EDGE-001..008, NFR-PRIV-004
- **Sustituye a:** Enmienda T-22 de `10_SECURITY_PRIVACY.md`

## Contexto

La decisión **D-03** del propietario (`GS-ENG-A02` §3) establece que GeoStream debe localizar incluso dispositivos tipo AirTag sin conexión a internet (clase **E-BLE**), además de admitir rastreadores LPWAN (**E-LPWAN**) y celulares (**E-CELL**). Para localizar un tag BLE sin internet sin que el servidor ni los teléfonos buscadores puedan espiar la ubicación del tag ni ser vinculados entre sí, y evitando que los tags se usen para acoso (*stalking*), el protocolo debe ser cifrado de extremo a extremo (E2E) y cumplir con las protecciones contra rastreo no deseado.

## Opciones consideradas

1. Anunciar un ID fijo en BLE y enviar reportes en claro al servidor (permite a cualquiera rastrear el tag y expone todas las ubicaciones al servidor).
2. Intentar conectarse directamente a las redes cerradas Find My de Apple o Find Hub de Google desde el backend (no existe API pública de consulta para terceros; son programas de certificación de hardware para fabricantes).
3. **Diseñar la red de búsqueda propia GeoStream Beacon en `gs-edge` (`no_std`) con identificadores efímeros rotatorios sobre P-256, cifrado híbrido E2E (ECDH P-256 + HKDF-SHA256 + AES-256-GCM), reportes anónimos sin identidad del buscador y detección obligatoria de rastreo no deseado alineada con IETF DULT.**

## Decisión

Adoptar la Opción 3 (decisión D-03):
- **Aprovisionamiento sin conocimiento del servidor (`FR-EDGE-001`):** el dispositivo dueño genera una clave de identidad efímera `EIK` de 32 bytes aleatorios, la instala en el tag vía BLE y registra el tag en GeoStream (`tag_…`) enviando solo metadatos. El servidor **nunca** recibe el `EIK`.
- **Rotación de EID:** cada ventana `c = floor(t / edge_rotation_s)` (inicial 1024 s), el tag deriva el escalar `r = KDF(EIK, c) mod n` en P-256 y anuncia la coordenada `x` del punto `R = r·G` (32 bytes).
- **Trama BLE y compatibilidad iOS:** anuncio conectable con el UUID de servicio de GeoStream (requisito de iOS en segundo plano). El spike **T-E8-01** verificará en dispositivos físicos Android e iOS la recepción en segundo plano de anuncios extendidos BLE 5 frente a anuncio *legacy* conectable con lectura GATT breve.
- **Reporte cifrado E2E y no vinculable (`FR-EDGE-002`, `NFR-PRIV-004`):** el buscador reconstruye `R`, genera un par efímero `(e, E = e·G)`, deriva la clave con `HKDF-SHA256(ECDH(e, R), info="gs-edge-v1" ‖ EID)`, cifra `{lat_e7, lon_e7, accuracy_m, observed_at}` con **AES-256-GCM** y sube `{hashed_eid = SHA-256(EID)[0..16], time_bucket, E, nonce, ciphertext}` a `POST /v1/edge/reports`. El servidor no persiste el `device_id` ni `subject_id` del buscador junto al reporte.
- **Protección contra rastreo no deseado (`FR-EDGE-004`):** estado *separado* en el tag tras `edge_separated_after_h` (rotación de EID a 24 h, sonido y desactivación física) y detector DULT en `gs-client-core` que emite `UnwantedTrackerAlert` al observar un tag separado desconocido a lo largo del desplazamiento del usuario.

## Consecuencias

+ El servidor almacena únicamente blobs opacos con TTL `edge_report_ttl_hours` y no puede conocer la posición de los tags (enmienda T-22 para tags Beacon).
+ `gs-edge` es 100 % `no_std`, reutilizable tanto en `gs-client-core` como en el firmware del tag.
- Su cobertura depende de la densidad de dispositivos con el SDK del mismo tenant (`edge_relay_scope = TENANT` por defecto), por lo que se documenta honestamente como red de flota/operador.

## Seguridad, privacidad y operación

Se añaden las amenazas T-26, T-27 y T-28 en `10_SECURITY_PRIVACY.md`. Solo se componen primitivas estándar auditadas (`p256`, `hkdf`, `aes-gcm`, `sha2`), y el protocolo exige revisión criptográfica externa antes del primer release de producción de E8 (`T-E8-02`).

## Plan de migración

Especificado en `25_EDGE_DEVICES.md` (E0) e implementado en la fase E8 tras los spikes y puertas de seguridad.

## Evidencia y referencias

- Decisión D-03 en `docs/proposals/GeoStream_Addendum_A02_Motor_Ubicacion_Movilidad.md` §3 y §12.
- Google Find Hub Network Accessory Specification; IETF DULT (`draft-ietf-dult-threat-model`); guía de escaneo BLE en iOS (Anexo B de A-02).
