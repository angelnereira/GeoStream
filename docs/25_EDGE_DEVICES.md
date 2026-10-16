# 25 — Dispositivos edge y sin internet

**ID:** GS-EDG-001 · **Versión:** 0.3.0
Gobierna `gs-edge` (librería pura `no_std`) y la ingesta desde LPWAN y rastreadores celulares. Complementa `07` §9 (ingesta) y `10` §4 (T-22, T-26..T-28).

## 1. Clases de dispositivo

| Clase | Ejemplo | Conectividad propia | Cómo entra en GeoStream | Fase |
|---|---|---|---|---|
| **E-BLE** | Tag tipo AirTag (BLE, pila de botón) | Ninguna | Red de búsqueda **GeoStream Beacon**: teléfonos con el SDK relevan reportes cifrados E2E (§2) | E8 |
| **E-LPWAN** | Rastreador GPS LoRaWAN | Radio hacia un gateway | Webhook del network server → adaptador de ingesta (§4) | E8 |
| **E-CELL** | Rastreador GPS celular (2G, LTE-M, NB-IoT) | Celular | Ingesta HTTP (`FR-LOC-009`), MQTT (F14) o pasarela TCP con códecs de fabricante (COULD, §4) | F8 / F14 / E8 |
| **E-PHONE** | Teléfono sin datos durante un rato | Intermitente | Cola `buffered` cifrada (`22` §3.5, ADR-0025) | E3 / E6 |

## 2. Red de búsqueda GeoStream Beacon (E-BLE)

El diseño se alinea con dos referencias públicas, sin depender de ninguna red propietaria: la especificación del accesorio de la red Find Hub de Google (identificadores efímeros derivados de una clave del dueño, rotación periódica, cifrado del reporte hacia la clave pública anunciada, backend que no puede leer ubicaciones) y los borradores del grupo de trabajo IETF **DULT**.

### 2.1 Roles

- **Tag:** accesorio BLE. Clave `EIK` en el firmware; anuncia solo su `EID`.
- **Dueño:** dispositivo con el SDK que registró el tag. Es el único que posee el `EIK`.
- **Buscador (*finder*):** cualquier dispositivo con el SDK que participe como relevo.

**Alcance de la red** (`edge_relay_scope`):
- `TENANT` (por defecto): solo relevan dispositivos del mismo tenant. Por ejemplo, la flota de un operador encuentra sus propios paquetes, bolsos o patinetes.
- `NETWORK`: relevo entre tenants que lo acepten expresamente. Requiere consentimiento de relevo del buscador y un ADR propio.

### 2.2 Aprovisionamiento

1. El dueño genera `EIK` (32 bytes aleatorios) y lo escribe en el tag por un canal BLE autenticado.
2. Registra el tag en el servidor (`tag_…`) con **solo metadatos**: modelo, nombre y subject dueño. El servidor **nunca** recibe el `EIK`, ni en el alta ni en ninguna operación posterior (`FR-EDGE-001`).

### 2.3 Identificador efímero

- Contador `c = floor(t / edge_rotation_s)`, con `edge_rotation_s = 1024` inicial.
- Escalar `r = KDF(EIK, c) mod n` en P-256. Punto `R = r·G`.
- **EID = coordenada x de R** (32 bytes).

La KDF exacta, la reconstrucción del punto a partir de la coordenada x (elección de la paridad de `y`) y el formato de trama los fija **ADR-0029** tras el spike T-E8-01, y esa parte **requiere revisión externa**.

### 2.4 Trama BLE

- Con 32 bytes, el EID requiere **anuncio extendido BLE 5**. La variante *legacy* de Find Hub usa una curva de 160 bits (SECP160R1); no se adopta salvo que el agente encuentre una implementación auditada y mantenida, lo que debe verificar y registrar en ADR-0029.
- Para que iOS detecte el tag en segundo plano, el anuncio DEBE ser *conectable* e incluir el UUID de servicio de GeoStream. iOS descarta en segundo plano los anuncios no conectables y los que no coinciden con un filtro de UUID.
- **Spike obligatorio (T-E8-01):** comprobar en dispositivos reales que Android e iOS reciben el formato elegido en segundo plano. Si iOS no recibe anuncios extendidos en segundo plano, se evalúa la alternativa: anuncio *legacy* conectable con un identificador corto de 20 bytes y lectura GATT breve del EID. Esta alternativa cuesta más batería y se decide con mediciones.

### 2.5 Reporte del buscador

1. Con su propia posición (precisión ≤ `edge_finder_max_accuracy_m`), el buscador arma `{lat_e7, lon_e7, accuracy_m (+ margen de alcance BLE según RSSI), observed_at (resolución de minutos)}`.
2. Cifra: ECDH P-256 efímero contra `R` → HKDF-SHA256 (info = `"gs-edge-v1" ‖ EID`) → **AES-256-GCM**.
3. Envía `POST /v1/edge/reports` (scope `edge:report`) con `{hashed_eid = SHA-256(EID)[0..16], time_bucket (hora), finder_ephemeral_pub, nonce, ciphertext}`.

### 2.6 Almacenamiento en el servidor

- Blobs opacos con TTL `edge_report_ttl_hours`.
- **No se guarda la identidad del buscador** junto al reporte: ni `device_id`, ni `subject_id`, ni la IP de origen en el registro del reporte. Solo se lleva un contador de tasa con TTL corto, de modo que los reportes no se pueden vincular entre sí (T-28, `NFR-PRIV-004`).

### 2.7 Consulta del dueño

1. El dispositivo dueño deriva los EID de la ventana de tiempo y llama a `POST /v1/edge/reports:lookup` (scope `edge:lookup`, como máximo `edge_lookup_max_ids` IDs, con rate limit).
2. Descifra en el núcleo.
3. Fusiona varios reportes, ponderados por precisión, en `TagLocation {position, accuracy_m, observed_at, report_count}`, descartando atípicos (T-27).

### 2.8 Puente opcional (COULD)

El dueño puede republicar la ubicación descifrada como un **dispositivo virtual** de su subject (token emitido para el tag por su backend, `source = EXTERNAL_DEVICE`). Así el tag entra en sesiones, geocercas e historial.

Es una decisión explícita del dueño: a partir de ahí **el servidor sí ve esas coordenadas**, y pasa a regir la política de retención del historial (`history_mode`, `24` §1).

## 3. Protección contra rastreo no deseado (obligatoria antes de cualquier release de E8)

**Tag (firmware):**
- Estado *separado* cuando el dueño lleva tiempo sin estar cerca (`edge_separated_after_h`), con rotación del identificador que se ralentiza a 24 h, como el modo UTP de Find Hub.
- Sonido a pedido y servicio de identificación para no dueños.
- Forma de desactivar el anuncio controlada por el usuario, sin restablecer de fábrica.

**SDK del buscador:** algoritmo de detección. Si un tag desconocido en estado separado se observa `≥ dult_min_sightings` veces, en `≥ dult_min_distinct_locations` lugares que abarcan `≥ dult_min_distance_m`, durante `≥ dult_min_duration_min`, mientras el usuario se desplaza, se emite `UnwantedTrackerAlert` al anfitrión con instrucciones (hacer sonar, desactivar).

**Seguimiento de los borradores:** los parámetros siguen los borradores vigentes de DULT (protocolo de accesorio y algoritmos de detección), que siguen en evolución. ADR-0029 fija la versión de borrador seguida y su revisión periódica.

**Revisión externa:** el protocolo criptográfico requiere revisión externa antes del primer release. `10` §5.6 ya prohíbe primitivas propias, y aquí solo se componen primitivas estándar (P-256, HKDF-SHA256, AES-256-GCM, SHA-256).

## 4. LPWAN y rastreadores celulares

**LPWAN:** `POST /v1/ingest/lpwan/{integration_id}` recibe el webhook HTTP del network server.
- Se autentica con un secreto de integración (HMAC, como los webhooks salientes, `07` §10.3).
- Los decodificadores por modelo de dispositivo implementan un trait de `gs-edge`: `decode(fport, bytes) -> Vec<RawLocationSample>`.
- La asociación `DevEUI → dispositivo GeoStream` se registra por REST (`lpwan:manage`).
- Las muestras pasan por las reglas normales de `09` §3 y por el motor de trayectoria (`22` §2).
- El agente verifica el formato del webhook de cada network server soportado antes de implementarlo (`20` §9).

**Pasarela de rastreadores (COULD):** rol `gateway` con un listener TCP y códecs de protocolos de rastreadores GPS comunes en flotas. Cada protocolo se implementa solo tras verificar su especificación oficial y con su ADR. La asociación `IMEI → dispositivo` se registra por REST.

## 5. Firmware de referencia (COULD)

- `firmware/gs-tag`: Rust `no_std` con `embassy` sobre Nordic nRF52840 o nRF54. Usa `gs-edge` para EID y tramas.
- Queda fuera del workspace del servidor; puede vivir en un repositorio aparte.
- Su consumo de batería se mide (`docs/device-tests/`); no se promete.

## 6. Límites que se documentan sin maquillar

- **Cobertura:** depende de la densidad de teléfonos con apps que integran el SDK. No es comparable con las redes de Apple o Google. Su valor está en las **redes privadas de flotas**.
- **Interoperabilidad:** unirse a la red Find My de Apple (programa de accesorios dentro de MFi) o a la red Find Hub de Google (registro en Nearby Device Console y certificación) es posible para fabricantes de tags, pero queda **fuera de GeoStream**.
- **iOS:** en segundo plano exige filtro de UUID de servicio y anuncios conectables, aplica filtrado de duplicados y escanea con intervalos largos. Los buscadores iOS detectan con menos frecuencia, y Android lleva la mayor parte del relevo.
- **Batería del buscador:** el escaneo se hace solo cuando la app ya está activa en segundo plano por ubicación y con un ciclo de trabajo bajo (`edge_scan_duty_cycle`). El costo se mide (`NFR-MOB-002`).

## 7. Qué ve cada parte (`10` §6.3)

| Parte | Qué ve |
|---|---|
| **Buscador Beacon** | Solo el EID que escucha, y su propia posición. Nunca la ubicación del tag ni la de otros usuarios. |
| **Servidor** | Nada de las ubicaciones de tags (solo blobs opacos), salvo que el dueño use el puente del §2.8. |
| **Dueño** | Las posiciones descifradas de sus propios tags, fusionadas por precisión. |
| **Proveedor** | Nada: la red Beacon no usa proveedores externos. |

## 8. Vectores y escenarios

- Vectores `EDGE-EID` (derivación con `EIK` y contador fijos) y `EDGE-RPT` (cifrado con claves efímeras fijas), con `EIK` y claves sintéticos.
- Escenario `SCN-025`: reporte cifrado → lookup → descifrado; el servidor no puede leerlo y no guarda al buscador.
- Ninguna coordenada real de personas se usa en vectores ni escenarios (`11` §6).

## 9. Referencias

- `03` §2.22 (FR-EDGE-*), `03` §3 (NFR-PRIV-004).
- `10` §4 (T-22 enmendada, T-26, T-27, T-28, T-31), `10` §5.6 (cifrado).
- `17` §4 (política de edge) y §2 (parámetros del servidor).
- ADR-0029.
