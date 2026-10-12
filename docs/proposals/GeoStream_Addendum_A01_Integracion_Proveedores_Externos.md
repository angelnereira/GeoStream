# GeoStream — Addendum A-01: Integración de servicios de terceros

**ID propuesto:** GS-EXT-001 · **Estado:** Propuesta (no normativa hasta que se adopte) · **Fecha:** 2026-10-12
**Base:** repositorio `v0.2.0` (`docs/13_MAPS_PROVIDERS.md`, ADR-0013, FR-MAP-001..006, NFR-RES-001, NFR-COST-001)
**Insumo externo:** documento "Diseño técnico: servicio universal de geolocalización" (v1.0, octubre 2026) aportado por el propietario, más verificación en fuentes oficiales hecha para este addendum (§3 y Anexo C).

> Este documento **no modifica** ningún archivo del repositorio. Para adoptar un punto hay que convertirlo en requisitos (`docs/03`), tareas (`docs/21`) y ADR, siguiendo `docs/00_INDEX.md`. La sección 11 lista exactamente qué tocar.

---

## 1. Qué resuelve

Hoy hay dos opciones de proveedor (MapLibre y Google Maps) y el diseño de `13` está escrito alrededor de ellas. Este addendum define **cómo se acopla un proveedor nuevo sin tocar el núcleo**, cómo se garantiza el cumplimiento de sus términos por construcción y dónde corre cada integración (en el dispositivo o en el servidor).

Tesis central, en tres decisiones:

1. **Puertos y adaptadores con manifiesto.** Cada proveedor es un paquete que implementa uno o más *puertos* (interfaces) y trae un *manifiesto* declarativo con sus capacidades, términos y límites. Las reglas de compatibilidad dejan de estar escritas en código (`if google …`) y pasan a evaluarse desde los manifiestos.
2. **Todo resultado de un proveedor lleva su procedencia.** Cada geocodificación, ruta o lugar viaja con una `ContentPolicy` (quién lo generó, dónde puede mostrarse, hasta cuándo puede guardarse, qué atribución exige). Las cachés y exportaciones la respetan.
3. **Dos planos de ejecución.** v1 mantiene los adaptadores **en el dispositivo** (decisión vigente). Un **Provider Gateway** del lado servidor queda definido como módulo opcional posterior, desactivado por defecto, con claves aportadas por cada integrador (BYOK).

## 2. Qué aporta la investigación externa y cómo se reconcilia

| Propuesta del documento externo | Decisión | Motivo |
|---|---|---|
| Proveedor intercambiable; Google como adaptador, no como dueño de la lógica | **Adoptada** (ya es ADR-0013) | Se refuerza con manifiestos y reglas como datos (§4). |
| Principio "no todo pasa por el backend": mapa visual con SDK nativo, GPS en el dispositivo | **Adoptada** | Coincide con `13` §1 y FR-SDK-016. |
| Módulos Maps, GPS, Places, Routes, Geocoding, Offline | **Adaptada** | Maps/Routes/Geocoding ya existen como puertos; **Places** se añade (§4.2); Offline se trata como capacidad de teselas (§4.2), no como almacén de historial. |
| Tres niveles de capacidad offline (propio / mapas sin conexión / dependiente de proveedor) | **Adoptada como modelo de capacidades** | Cada capacidad declara su nivel offline en el manifiesto (`offline`). |
| Proxy en el backend de Places/Routes/Geocoding (REST `/v1/places`, `/v1/routes`, `/v1/geocoding`) | **Diferida** a un Provider Gateway opcional (§5) | Contradice `13` §8 (el servidor v1 no usa proveedores) y tiene implicaciones de licencias y privacidad que exigen ADR propio. |
| `GET /v1/providers` (capacidades disponibles para el cliente) | **Adaptada** | Solo tiene sentido con el gateway; mientras tanto, el descubrimiento es local (el stack declarado en la app). |
| Tabla `provider_usage` y medición por SKU | **Adoptada con el gateway** y con la brecha G-08 | Las métricas Prometheus no pueden llevar `tenant` (ADR-0017), así que el consumo por tenant va a PostgreSQL agregado. |
| Presupuesto y escenario de costos | **Adoptada como método**, cifras de Google contrastadas en §3 | Ver §3.3. |
| BYOK / separación de credenciales públicas y privadas | **Adoptada** (§6) | Define quién es el "Customer" ante el proveedor. |
| PostGIS, `location_events`, historial propio | **No adoptada en v1** | Contradice ADR-0005 (efímero por defecto) y NFR-PRIV-001. Se reevalúa en F13. |
| Cola offline con SQLite y sincronización | **No adoptada en v1** | Contradice ADR-0014 (`latestOnly`). Es la pista F13 (FR-HIS-002). |
| gRPC | **Diferida** | Sin necesidad concreta; REST y WSS cubren v1. |
| OIDC/OAuth 2.0 de clientes | **Diferida a F15** (FR-AUTH-009) | El modelo vigente son API keys + tokens de dispositivo. |
| "Decidir si es biblioteca o plataforma multi-tenant" | **Ya decidido** | El diseño es multi-tenant (ADR-0006). |

## 3. Hallazgos verificados

Fecha de consulta: **2026-10-12**. Las cifras y cláusulas se citan de las fuentes oficiales del Anexo C; todo lo no verificado se marca así.

### 3.1 Google Maps Platform — términos específicos (última modificación 10-jun-2026)

- **Uso sin mapa permitido, con mapa ajeno prohibido.** Geocoding (6.1/6.2), Routes (19.1/19.2), Places (14.1/14.2), Distance Matrix (5.1/5.2), Roads (17.1/17.2) y Geolocation (7.1/7.2): se puede usar el contenido *sin* un mapa de Google, pero no *junto con* un mapa que no sea de Google.
- **Excepción:** Places UI Kit (15.1) puede usarse con o sin mapa, incluido uno que no sea de Google. Es la única vía documentada para mostrar contenido de Places sobre MapLibre. Queda por verificar si existe soporte utilizable desde Flutter.
- **Caché.** Latitud/longitud: hasta 30 días consecutivos en Geocoding (6.3.1), Routes (19.3), Places (14.3), Roads, Geolocation y otros. **Excepción de Geocoding (6.3.2):** `lat/lng`, `formatted_address` y valores estructurados pueden guardarse sin límite solo para sostener la funcionalidad directa al usuario final que originó la petición, nunca como sustituto de una nueva llamada y aislados lógicamente por usuario final. Los IDs de Google (p. ej. `place_id`) pueden guardarse según la documentación (A.3).
- **Atribución.** Obligatoria según la documentación del servicio (A.2); no puede quitarse ni oscurecerse (p. ej. 15.4 para Places UI Kit).
- **No existe una cláusula explícita de "base de datos derivada"** en ese documento, pero las cláusulas anteriores impiden construir una base geográfica propia con contenido de Google.

### 3.2 Otros proveedores y servidores públicos

| Servicio | Hallazgo verificado | Consecuencia |
|---|---|---|
| Nominatim público (`nominatim.openstreetmap.org`) | Máximo **1 solicitud por segundo** por aplicación; `User-Agent` o `Referer` que identifique la app; **autocompletado prohibido**; geocodificación masiva desaconsejada; sin garantía y puede retirarse el acceso. | Solo desarrollo y volumen mínimo. Producción: instancia propia o proveedor comercial. |
| Teselas OSM estándar (`tile.openstreetmap.org`) | Uso interactivo permitido; **prohibidos** la descarga masiva, el prefetch y las **funciones offline**; sin SLA; servicios comerciales deben esperar que se retire el acceso; atribución visible. | Solo desarrollo. Nunca base para teselas offline. |
| OSRM público (demo) | Uso "razonable y **no comercial**"; **1 solicitud por segundo**; sin garantías. | Solo desarrollo. En producción, instancia propia. |
| Mapbox | Los términos generales indican que las aplicaciones de **vehículos** requieren licencia comercial separada y que el integrador debe trasladar a sus usuarios el aviso de direcciones. Las condiciones de producto (almacenamiento, combinación con mapas ajenos) **no se leyeron**. | GeoStream apunta a flotas y reparto: tratar Mapbox como **no verificado** hasta leer sus Product Terms. |
| Azure Maps, HERE, TomTom, MapTiler, Stadia | **No verificados en esta ronda** (solo hubo fuentes secundarias). | No pueden marcarse como `stable` (regla EXT-R-07). |

### 3.3 Precios de Google contrastados

La página oficial de precios (actualizada el 2026-10-08) confirma la estructura Essentials/Pro/Enterprise con cupo gratuito mensual por SKU y avisa de **cambios próximos en descuentos, créditos y paso de servicios a Legacy**. Los valores usados por el documento externo cuadran:

| SKU | Cupo gratuito | Primer tramo (por 1 000) |
|---|---|---|
| Geocoding (Essentials) | 10 000 | USD 5,00 |
| Compute Routes Essentials | 10 000 | USD 5,00 |
| Place Details Essentials | 10 000 | USD 5,00 |
| Dynamic Maps (Essentials) | 10 000 | USD 7,00 |
| Maps SDK (nativo móvil) | ilimitado | USD 0 |

Recalculado: 40 000 × 5/1 000 + 10 000 × 5/1 000 + 20 000 × 5/1 000 = **USD 350/mes**, y 90 000 cargas web × 7/1 000 = **USD 630**, igual que en el documento externo. Advertencias: el escenario no incluye Text Search (Pro, USD 32 por 1 000), Autocomplete ni matrices, y las cifras cambian; por eso el manifiesto lleva `pricing_checked_on` y es informativo, nunca normativo.

### 3.4 Paquetes Flutter (resuelve en parte lo pendiente de T-F5-01 y T-F9-02)

| Paquete | Versión (pub.dev) | Publicado | Licencia | Estado |
|---|---|---|---|---|
| `geolocator` | 14.1.1 | 2026-09-27 | MIT (Baseflow) | Verificada |
| `maplibre_gl` | 0.27.1 | 2026-09-10 | BSD-3-Clause (archivo de licencia del repositorio, mantenedores MapLibre) | Verificada en el repositorio |
| `maplibre` (otro paquete, `josxha`) | 0.3.6 | 2026-08-25 | **No verificada** | Candidato alternativo |

Hay dos paquetes MapLibre para Flutter. La elección se hace en T-F9-02 con una prueba en dispositivo; el puerto `MapRenderer` hace que la elección sea reversible.

## 4. Modelo de integración de proveedores

### 4.1 Vocabulario

| Término | Definición |
|---|---|
| **Capacidad** | Lo que un proveedor sabe hacer, independiente del proveedor: `map.render`, `map.tiles`, `geo.forward`, `geo.reverse`, `geo.autocomplete`, `places.search`, `places.details`, `route.compute`, `route.matrix`, `route.snap` (map-matching). |
| **Puerto** | Interfaz de GeoStream para una capacidad (§4.2). |
| **Adaptador** | Paquete que implementa uno o más puertos para un proveedor concreto. |
| **Manifiesto** | Archivo declarativo del adaptador con capacidades, términos, límites y datos que salen del dispositivo (§4.3). |
| **Pila (`ProviderStack`)** | Conjunto de adaptadores elegidos por el integrador. Se valida contra los manifiestos al construirse. |
| **`ContentPolicy`** | Metadatos de procedencia y derechos que acompañan a cada resultado (§4.4). |

### 4.2 Puertos (cambios respecto a `13` §3)

Lo que **se mantiene**: `MapRenderer`, `GeocodingProvider`, `RoutingProvider`, `ProviderStack`, `ProviderException`.

Lo que **cambia**:

1. **`ProviderId` deja de ser un `enum` cerrado.** Hoy `enum ProviderId { google, maplibre, mapbox, here, selfHosted, osmCommunity }` obliga a editar el núcleo para añadir un proveedor. Se reemplaza por un tipo extensible:
   ```dart
   extension type const ProviderId(String value) {
     static const google = ProviderId('google');
     static const maplibre = ProviderId('maplibre');
     static const osmCommunity = ProviderId('osm-community');
     // Un adaptador de terceros define el suyo sin tocar geostream_maps:
     // static const acme = ProviderId('acme');
   }
   ```
2. **Se separa el *renderer* de la *fuente de teselas*.** `maplibre` es un renderer, no un proveedor de datos: necesita un `styleUrl` y teselas de alguien (propias, MapTiler, Stadia, OSM comunitario…). Se añade el puerto `TileSource` y `ProviderStack` pasa a tener `tiles` como pieza propia. Las reglas de licencia de teselas (p. ej. OSM comunitario) se aplican a esa pieza.
3. **Se añaden puertos nuevos** (firmas orientativas; las definitivas quedan en el contrato del paquete):
   ```dart
   abstract interface class PlacesProvider {
     ProviderId get provider;
     Future<List<PlaceResult>> search(PlaceQuery query);   // cancelable
     Future<PlaceResult> details(PlaceRef ref, {Set<PlaceField>? fields});
   }
   abstract interface class MatrixProvider {
     ProviderId get provider;
     Future<MatrixResult> matrix(List<Waypoint> origins, List<Waypoint> destinations,
         {TravelMode mode = TravelMode.driving});
   }
   abstract interface class TileSource {
     ProviderId get provider;
     String get styleUrl;                 // o TileJSON
     OfflinePolicy get offlinePolicy;     // allowed | forbidden (de su manifiesto)
   }
   ```
4. **Autocompletado como capacidad separada** (`geo.autocomplete`), porque algunos proveedores lo prohíben (Nominatim público) o lo facturan de otra forma (Google, por sesión).
5. **Todo resultado devuelve su `ContentPolicy`** (§4.4).

### 4.3 Manifiesto del adaptador

Ubicación propuesta: `contracts/providers/<id>.manifest.yaml`, validado por `contracts/providers/manifest.schema.json`. El manifiesto es **dato versionado**: una incoherencia con los términos se corrige editando el manifiesto, no el código.

Campos obligatorios:

| Campo | Contenido |
|---|---|
| `id`, `name`, `version`, `package` | Identidad del adaptador. |
| `plane` | `client` (corre en la app) o `server` (corre en el gateway). |
| `capabilities` | Lista de capacidades (§4.1) con, por cada una, `offline`: `none`, `cache-limited` o `full`. |
| `terms` | `url`, `checked_on` (fecha), `commercial_use` (`allowed`, `non_commercial_only`, `unverified`) y `community_server` (booleano). |
| `content` | Reglas sobre lo que devuelve: `display_only_with_renderer` (lista de proveedores), `headless_allowed`, `cache` (`latlng_max_days`, `ids_cacheable`, `exceptions`), `export_allowed`. |
| `attribution` | `required`, `text`, `placement`, `non_removable`. |
| `limits` | Cuotas conocidas (p. ej. `rate_limit_rps: 1`), informativas. |
| `forbidden` | Capacidades que el proveedor prohíbe (p. ej. `geo.autocomplete` en Nominatim público). |
| `auth` | `client_restricted_key`, `server_secret`, `oauth` o `none`. |
| `egress` | Qué datos salen del dispositivo hacia el proveedor (`viewport`, `query_text`, `coordinates`, `device_ip`). Alimenta el aviso de privacidad (`13` §7). |
| `pricing` | Informativo: unidad, cupo gratuito y `checked_on`. |
| `status` | `experimental`, `stable` o `deprecated`. |

Ejemplo completo en el Anexo A.

### 4.4 `ContentPolicy`

Cada `GeocodeResult`, `RouteResult`, `PlaceResult` y `MatrixResult` incorpora:

| Campo | Significado |
|---|---|
| `provider` | Quién lo generó. |
| `retrieved_at` | Cuándo se obtuvo. |
| `display_only_with` | Renderers con los que puede mostrarse (vacío = sin restricción). |
| `store_until` | Instante límite para conservar coordenadas (p. ej. `retrieved_at + 30 días` para Google); `null` si no hay límite. |
| `store_scope` | `none`, `end_user_only` (excepción 6.3.2) o `unrestricted`. |
| `attribution` | Texto y enlace a mostrar. |
| `redistributable` | Si puede enviarse a otros sistemas (webhooks, API). Por defecto **falso**. |

Consecuencias de implementación:
- Una caché del SDK (si el integrador la activa) borra por `store_until` y nunca guarda resultados con `store_scope: none`.
- Los resultados con `redistributable: false` **no se envían a GeoStream** (refuerza `13` §3: "los resultados de geocodificación y rutas no se envían a GeoStream").
- Un `ContentPolicy` ausente es un defecto del adaptador: la prueba de conformidad lo rechaza (§8.2).

### 4.5 Motor de cumplimiento (reglas como datos)

`ProviderStack` evalúa los manifiestos con una lista cerrada de reglas. Cada regla tiene ID, motivo y vector de prueba.

| Regla | Condición → resultado | Origen |
|---|---|---|
| EXT-R-01 | Si algún componente (`geocoding`, `routing`, `places`, `matrix`) declara `display_only_with_renderer` y el renderer no está en esa lista → `ConfigurationException`. | Google 5.2, 6.2, 14.2, 19.2 |
| EXT-R-02 | Si un componente tiene `community_server: true` y `allowCommunityServers == false` → `ConfigurationException`; en release con `true` se registra advertencia. | Políticas de Nominatim, teselas OSM y OSRM público |
| EXT-R-03 | Si se usa una capacidad listada en `forbidden` del proveedor (p. ej. `geo.autocomplete` en Nominatim público) → `ConfigurationException`. | Política de Nominatim |
| EXT-R-04 | Si la app activa paquetes offline de teselas y `TileSource.offlinePolicy == forbidden` → `ConfigurationException`. | Política de teselas OSM |
| EXT-R-05 | Todo proveedor con `attribution.required` exige que el renderer exponga atribución visible; prueba de widget obligatoria en su adaptador. | Google A.2; OSM |
| EXT-R-06 | CI: avisa si `terms.checked_on` supera 180 días y bloquea si supera 365. | `13` §2, FR-MAP-006 |
| EXT-R-07 | Un manifiesto con `commercial_use: unverified` no puede tener `status: stable`. | Mapbox, Azure Maps, HERE, TomTom sin verificar |
| EXT-R-08 | Si un componente tiene `headless_allowed: false`, una pila sin renderer que lo incluya es inválida. | Prudencia |

**Postura conservadora respecto a Google.** Los términos permiten usar Geocoding/Routes/Places "sin un mapa de Google", pero prohíben usarlos "junto con" uno ajeno. La frontera exacta de "en conjunto" no es evidente. Por eso EXT-R-01 mantiene la regla estricta de `13` §4 (renderer de Google obligatorio) y solo habilita un modo `headless` (sin renderer; texto, p. ej. mostrar una dirección en una lista) tras revisión legal explícita registrada en el manifiesto (`content.headless_allowed: true` con `legal_review_ref`).

## 5. Planos de ejecución

### 5.1 Plano cliente (v1, vigente)

Los adaptadores corren en la app. La app consulta al proveedor directamente con una clave restringida. GeoStream no ve la consulta ni su resultado.

| A favor | En contra |
|---|---|
| El servidor sigue sin dependencias externas (NFR-COST-001, NFR-RES-001). | La clave del proveedor vive en la app (se restringe por paquete/bundle, pero se puede extraer). |
| Privacidad: la consulta no pasa por GeoStream. | Sin cuotas ni medición centralizadas por tenant. |
| Cumplimiento más simple: el integrador es el "Customer" ante el proveedor. | Sin conmutación por error común entre apps. |

### 5.2 Plano servidor: Provider Gateway (opcional, posterior)

Módulo del monolito (crate `gs-providers`, rol `gateway`) que expone `/v1/geo/*` para geocodificación, rutas, matrices y lugares. **Desactivado por defecto por tenant**; no participa del camino crítico de ubicación (mismo aislamiento que las geocercas).

Cuándo se justifica:
- El integrador no puede llevar claves a la app (política de seguridad).
- Necesita cuotas y medición por tenant o por usuario final.
- Quiere un único punto con caché permitida, conmutación por error y métricas.

Condiciones obligatorias (todas):
1. **BYOK:** la clave del proveedor la aporta el integrador y se guarda cifrada por tenant. Un modo con clave de la plataforma queda fuera hasta tener asesoría legal (ver §6).
2. **Sin coordenadas en logs, métricas ni trazas** (FR-PRV-004): los tipos redactados cubren también las consultas al proveedor.
3. **Las reglas EXT-R-0x se evalúan en el servidor** con los mismos vectores de prueba que en el SDK.
4. **No se retransmite contenido con `redistributable: false`** a clientes que lo mostrarían en un renderer no permitido: el gateway devuelve el `ContentPolicy` y el SDK vuelve a evaluar la pila.
5. **Errores nuevos** en el catálogo (se actualizan a la vez `common.proto`, OpenAPI, `09` §6 y el SDK, como exige la regla de las cuatro copias): `PROVIDER_UNAVAILABLE`, `PROVIDER_QUOTA_EXCEEDED` y `PROVIDER_NOT_PERMITTED`.
6. **Claves de configuración nuevas** en `17` (todas con prefijo `GS_PROVIDERS_*`): activación global, timeouts, límite de solicitudes concurrentes por proveedor y TTL máximo de caché.

Estado: **diseño reservado, sin tareas de implementación en v1**. Requiere ADR-0021 antes de cualquier código.

## 6. Credenciales y custodia

| Clase | Ejemplo | Dónde vive | Quién la aporta |
|---|---|---|---|
| Clave pública restringida | Clave del SDK de Google Maps (por paquete Android / bundle iOS) | Configuración de la app del integrador | Integrador |
| Clave de estilo/teselas | Clave de MapTiler o Stadia restringida por app | Configuración de la app | Integrador |
| Secreto de servidor | Clave de Routes/Geocoding para el gateway | Gestor de secretos; cifrado por tenant | Integrador (BYOK) |
| Credencial de plataforma | Cuenta propia de GeoStream ante un proveedor | Gestor de secretos del operador | Operador |

**Por qué BYOK por defecto.** Los términos de Google se firman con el "Customer" que construye la aplicación y es quien responde por el uso final, la atribución y la facturación. Si GeoStream atiende a muchos integradores con **una sola clave de plataforma**, el operador pasaría a ser ese "Customer" frente a Google por todas las aplicaciones. Si el modelo de proxy con clave de plataforma es viable bajo esos términos **no está verificado**: requiere revisión legal. Mientras tanto, BYOK deja la relación contractual con cada integrador.

Reglas que ya rigen y se mantienen (`AGENTS.md`, NFR-SEC-003): ninguna clave de servidor en el SDK ni en el repositorio; ninguna clave en logs.

## 7. Resiliencia, cuotas, costos y observabilidad

- **Aislamiento (NFR-RES-001):** un fallo, lentitud o corte de un proveedor **nunca** afecta a la compartición ni al stream. Todo error de proveedor se envuelve en `ProviderException` con subtipos `ProviderUnavailable`, `ProviderQuotaExceeded`, `ProviderAuthFailed`, `ProviderUnsupported` y `ProviderTermsViolation` (este último solo en configuración).
- **Timeouts y reintentos:** cada puerto define un tiempo máximo por operación; los reintentos con backoff exponencial y jitter aplican solo a operaciones idempotentes y marcadas reintentables en el manifiesto.
- **Disyuntor (circuit breaker)** por adaptador y operación.
- **Conmutación por error (failover):** solo entre adaptadores cuyo manifiesto declare compatibilidad mutua y que pasen EXT-R-0x. Ejemplo válido: Google → Google (otro endpoint). Ejemplo **inválido por defecto**: Google Geocoding → Nominatim mientras se muestra un mapa de Google. La lista de conmutación se declara en la pila; no es implícita.
- **Medición:** cada llamada genera un evento de uso `{provider, operation, units, sku_estimate?}`. En el SDK se expone como callback al integrador; en el gateway se agrega por hora en PostgreSQL por tenant. Las métricas Prometheus solo llevan `provider` y `operation` como etiquetas (nunca tenant, sujeto ni sesión: ADR-0017).
- **Presupuesto:** cuota por tenant y por proveedor con interruptor de corte y alerta al 80 %. Aplica al gateway; en el plano cliente es responsabilidad del integrador.
- **Costos:** el manifiesto trae `pricing` informativo con fecha. Ningún comportamiento del sistema depende de esas cifras.
- **Auditoría:** la activación del gateway, el alta de una clave y su rotación se registran como acciones sensibles.

## 8. Cómo añadir un proveedor

### 8.1 Receta

1. **Elegir capacidades** y puertos que implementará. Si falta un puerto, proponerlo antes (§4.2); no ampliar un puerto con métodos exclusivos de un proveedor.
2. **Leer los términos vigentes** y registrar URL, fecha, y por cada capacidad: caché, atribución, combinación con otros mapas, uso comercial, uso offline y datos que salen del dispositivo.
3. **Escribir el manifiesto** y pasar el validador de esquema.
4. **Implementar el adaptador** en un paquete aparte (`geostream_maps_<id>` o `geostream_geo_<id>`): solo depende de `geostream_maps`, nunca del núcleo `geostream`.
5. **Devolver `ContentPolicy`** en todos los resultados.
6. **Normalizar errores** a la jerarquía `ProviderException`.
7. **Pasar el kit de conformidad** (§8.2) con respuestas grabadas (sin llamadas reales en CI).
8. **Añadir casos al vector de reglas** (`provider_stack_rules.json`) para las combinaciones que el nuevo proveedor habilita o prohíbe.
9. **Actualizar `13` §4** (matriz) con la fecha de consulta y la guía de integración (`15`).
10. **Registrar un ADR** solo si introduce una clase nueva de proveedor o un plano nuevo; un adaptador más de una clase conocida no requiere ADR.
11. **Mantenerlo `experimental`** hasta cumplir EXT-R-07 y tener una prueba en dispositivo real.

### 8.2 Kit de conformidad (`geostream_maps_testing`)

Suite reutilizable que cada adaptador ejecuta con sus propias respuestas grabadas:

| Prueba | Verifica |
|---|---|
| Contrato de puerto | Firmas, cancelación, orden de resultados, idioma y sesgo geográfico. |
| Normalización | Campos obligatorios presentes aunque el proveedor no los devuelva (valores `null` explícitos, nunca inventados). |
| `ContentPolicy` | Presente y coherente con el manifiesto en todos los resultados. |
| Errores | Cada fallo del proveedor (401, 403, 429, 5xx, timeout, respuesta mal formada) se traduce al subtipo correcto. |
| Secretos | Ninguna clave aparece en mensajes de error, registros ni en el `toString` de objetos. |
| Coordenadas | Ninguna coordenada se escribe en registros. |
| Atribución | Prueba de widget: visible y no ocultable por configuración. |
| Aislamiento | Con el adaptador fallando, el cliente de ubicación sigue compartiendo y recibiendo (prueba con adaptador que falla, T-F9-06). |

### 8.3 Revalidación periódica

Los términos cambian (los de Google tuvieron tres versiones en junio de 2026). EXT-R-06 convierte la revisión en una tarea de mantenimiento con fecha, no en una buena intención.

## 9. Otros servicios de terceros con el mismo marco

El mismo esquema (puerto + adaptador + manifiesto) sirve para dependencias que no son mapas. Se listan para dejar el diseño listo, no para implementarlas ahora.

| Servicio | Puerto | Plano | Estado y vínculo |
|---|---|---|---|
| Notificaciones push (FCM, APNs) | `PushSender` | Servidor (worker) o backend del integrador | Decisión pendiente D-6 (brecha G-07). Por defecto, el backend del integrador envía el push a partir de un webhook; así GeoStream no guarda credenciales de push. |
| Identidad federada (OIDC/JWKS) | `TokenVerifier` | Servidor | F15, FR-AUTH-009. |
| Gestor de secretos / KMS | `SecretStore` | Servidor | Hoy: sistema de archivos o gestor de secretos (`10`); formalizar el puerto con la brecha G-12. |
| Exportadores de observabilidad (OpenTelemetry) | — | Servidor | Ya cubierto por ADR-0017; el exportador es configuración, no adaptador. |
| Entrega de códigos de invitación (SMS/correo) | — | — | **Fuera de alcance:** la entrega la hace el integrador. |

## 10. Catálogo de candidatos

Estado de verificación al 2026-10-12. Ningún adaptador nuevo debe marcarse `stable` sin cumplir §8.1.

| Proveedor | Capacidades previstas | Plano | Verificación | Notas |
|---|---|---|---|---|
| Google Maps Platform | `map.render`, `geo.*`, `places.*`, `route.*` | Cliente | **Términos y precios verificados** | Adaptador SHOULD en F9. Reglas EXT-R-01/05. |
| MapLibre (renderer) + teselas propias/PMTiles | `map.render`, `map.tiles` | Cliente | Licencia de `maplibre_gl` verificada | Mejor privacidad; requiere elegir y alojar teselas. |
| Teselas comerciales (MapTiler, Stadia) | `map.tiles` | Cliente | **No verificado** | Revisar términos de offline, caché y atribución. |
| OSM comunitario (teselas, Nominatim, OSRM) | `map.tiles`, `geo.*`, `route.compute` | Cliente | **Políticas verificadas** | Solo desarrollo (EXT-R-02). |
| Nominatim / Pelias propios | `geo.forward`, `geo.reverse` | Cliente o servidor | No verificado (requiere operación propia) | Decidir si se justifica frente a un proveedor comercial. |
| Valhalla / OSRM propios | `route.compute`, `route.matrix`, `route.snap` | Servidor | No verificado | Map-matching es pista post-GA (FR-HIS-004). |
| Mapbox | `map.render`, `geo.*`, `route.*` | Cliente | **Parcial**: cláusula de vehículos | Relevante para flotas; leer Product Terms. |
| Azure Maps, HERE, TomTom | `geo.*`, `route.*`, `places.*` | Cliente o servidor | **No verificado** | Candidatos según el entorno del integrador. |

## 11. Cambios propuestos al repositorio

### 11.1 Requisitos nuevos (`docs/03_REQUIREMENTS.md`)

La familia `FR-PRV` ya existe (privacidad), por eso se propone `FR-EXT`.

| ID | Nivel | Fase | Requisito | Aceptación |
|---|---|---|---|---|
| FR-EXT-001 | MUST | F9 | `ProviderId` extensible; añadir un proveedor no modifica `geostream_maps`. | Un adaptador de prueba externo compila y se registra sin cambios en el paquete de interfaces. |
| FR-EXT-002 | MUST | F9 | Manifiesto de adaptador con esquema validado. | Todos los adaptadores del repositorio tienen manifiesto que pasa el validador. |
| FR-EXT-003 | MUST | F9 | `ProviderStack` evalúa reglas EXT-R-01..04 y 08 desde manifiestos. | Los vectores de `provider_stack_rules.json` pasan en Dart; combinaciones prohibidas lanzan `ConfigurationException`. |
| FR-EXT-004 | MUST | F9 | `ContentPolicy` en todo resultado de proveedor. | El kit de conformidad rechaza resultados sin ella. |
| FR-EXT-005 | MUST | F9 | Puerto `TileSource` separado del renderer; política offline por fuente. | Activar paquetes offline con OSM comunitario falla (EXT-R-04). |
| FR-EXT-006 | SHOULD | F9 | Puertos `PlacesProvider` y `MatrixProvider`. | Interfaces publicadas con adaptador simulado en `geostream_maps_testing`. |
| FR-EXT-007 | MUST | F9 | Kit de conformidad `geostream_maps_testing`. | El adaptador MapLibre y el de Google lo pasan en CI. |
| FR-EXT-008 | MUST | F9 | Verificación CI de fechas de términos (EXT-R-06) y de la regla EXT-R-07. | CI falla con manifiestos vencidos o `stable` con términos sin verificar. |
| FR-EXT-009 | COULD | Post-GA | Provider Gateway con BYOK, cuotas y medición por tenant. | Requiere ADR-0021 y ADR-0022; criterios definidos en ese ADR. |
| FR-EXT-010 | MUST | F9 | Resultados de proveedor con `redistributable: false` no se envían a GeoStream. | Prueba de contrato: el SDK no incluye geocodificación ni rutas en ningún mensaje de stream ni REST. |

### 11.2 ADR nuevos

| ADR | Título | Estado propuesto |
|---|---|---|
| ADR-0020 | Adaptadores con manifiesto, `ContentPolicy` y reglas de cumplimiento como datos | Propuesto (sustituye parcialmente a ADR-0013 §Decisión; ADR-0013 sigue vigente en lo demás) |
| ADR-0021 | Provider Gateway opcional del lado servidor | Propuesto, diferido |
| ADR-0022 | Credenciales de proveedores: BYOK por defecto | Propuesto, diferido |

### 11.3 Hoja de ruta (`docs/21_ROADMAP.md`)

Nuevas tareas dentro de F9, dependientes de T-F9-01:

| ID | Tarea | Requisitos | Depende de |
|---|---|---|---|
| T-F9-07 | Esquema de manifiesto y validador; manifiestos de MapLibre, Google y OSM comunitario | FR-EXT-002 | T-F9-01 |
| T-F9-08 | `ProviderId` extensible, `TileSource` y motor de reglas EXT-R-0x con vectores | FR-EXT-001, 003, 005 | T-F9-07 |
| T-F9-09 | `ContentPolicy` en los modelos de resultado | FR-EXT-004, 010 | T-F9-08 |
| T-F9-10 | Kit de conformidad `geostream_maps_testing` | FR-EXT-007 | T-F9-09 |
| T-F9-11 | Puertos `PlacesProvider` y `MatrixProvider` con adaptador simulado | FR-EXT-006 | T-F9-08 |
| T-F9-12 | Verificación CI de fechas y estado de términos | FR-EXT-008 | T-F9-07 |

T-F9-02, T-F9-04 y T-F9-05 se ajustan: el adaptador MapLibre y el de Google se entregan **con manifiesto, `ContentPolicy` y kit de conformidad**. El alcance de F9 pasa de 2–3 semanas a 3–4 (estimación sin base empírica).

Pista post-GA (nueva): **F17 — Provider Gateway** (FR-EXT-009), condicionada a ADR-0021/0022 y a la decisión D-3 del plan de completitud.

### 11.4 Documentos a modificar

| Documento | Cambio |
|---|---|
| `13_MAPS_PROVIDERS.md` | §2: añadir permisos 6.1/19.1, excepción 6.3.2 y excepción Places UI Kit 15.1; §3: puertos nuevos y `ProviderId` extensible; §4: regla de pila por manifiestos y matriz con las fechas de §3.2 de este addendum; §8: referencia al gateway. |
| `adr/ADR-0013` | Nota de relación con ADR-0020. |
| `12_FLUTTER_SDK.md` | Tabla de paquetes: `geostream_maps_testing`; `geolocator` 14.1.1 (MIT) verificado. |
| `19_ENGINEERING_STANDARDS.md` | Lista de dependencias: `maplibre_gl` con licencia BSD-3-Clause confirmada; decisión entre `maplibre_gl` y `maplibre` registrada en T-F9-02. |
| `15_INTEGRATION_GUIDE.md` | Sección "Elegir y combinar proveedores" con el aviso de egreso de datos y las advertencias de §3.2. |
| `10_SECURITY_PRIVACY.md` | Amenaza nueva: fuga de consultas o claves vía adaptador; el control es el kit de conformidad. |
| `17_CONFIGURATION.md` | Solo con el gateway (`GS_PROVIDERS_*`); nada en v1. |
| `contracts/` | `providers/manifest.schema.json`, `providers/*.manifest.yaml`, `test-vectors/provider_stack_rules.json`; el catálogo de errores solo cambia con el gateway. |
| `TRACEABILITY.md`, `PROGRESS.md`, `CHANGELOG.md` | Filas y entradas para los requisitos y tareas nuevos. |

### 11.5 Verificación

El verificador de consistencia (`verify.py`, hoy fuera del repositorio; ver brecha G-02) debe ampliarse para comprobar: IDs `FR-EXT-*` en `03`, `TRACEABILITY` y `21`; manifiestos contra el esquema; vectores de reglas contra el motor; fechas EXT-R-06.

## 12. Riesgos, supuestos y decisiones pendientes

**Riesgos**

| Riesgo | Mitigación |
|---|---|
| Los términos de un proveedor cambian tras publicar el adaptador. | EXT-R-06, kit de conformidad y revisión en cada PR. |
| Interpretación de "en conjunto con un mapa ajeno" (Google). | Regla estricta por defecto; excepciones solo con `legal_review_ref`. |
| Un adaptador filtra claves o coordenadas por registros de error. | Pruebas de secretos y coordenadas del kit de conformidad. |
| El gateway convierte a GeoStream en responsable de contenido de terceros. | BYOK, desactivado por defecto, ADR previo. |
| Proliferación de adaptadores sin mantenimiento. | `status` en el manifiesto; `experimental` no cuenta para la matriz de compatibilidad (NFR-COMPAT-001). |
| Las cifras de precios cambian pronto (la propia página de Google avisa de cambios). | Se tratan como informativas con fecha. |

**Supuestos**
- Las cláusulas se citan de la versión de los términos consultada; no se ha obtenido asesoría legal.
- `maplibre_gl` se asume adecuado hasta la prueba en dispositivo de T-F9-02.

**Decisiones del propietario**

| # | Decisión | Efecto |
|---|---|---|
| E-1 | ¿Se adopta ADR-0020 (manifiestos y reglas como datos) dentro de F9? | Cambia el alcance de F9 (+1 semana aprox.). |
| E-2 | ¿Se reserva el Provider Gateway como pista post-GA o se descarta? | Define si se escribe ADR-0021/0022. |
| E-3 | ¿Qué proveedores comerciales hay que verificar primero (Mapbox, Azure Maps, HERE, TomTom, MapTiler, Stadia)? | Prioriza la lectura de términos. |
| E-4 | ¿Se necesita revisión legal para el modo `headless` de Google y para un posible proxy con clave de plataforma? | Habilita o cierra esas variantes. |
| E-5 | ¿Cuál de los dos paquetes MapLibre se prueba primero? | Orden de T-F9-02. |

---

## Anexo A — Ejemplo de manifiesto

```yaml
# contracts/providers/google.manifest.yaml  (ilustrativo; los valores definitivos salen de la revisión de términos del PR)
id: google
name: Google Maps Platform
version: 1
package: geostream_maps_google
plane: client
status: experimental
capabilities:
  map.render:     { offline: none }
  geo.forward:    { offline: none }
  geo.reverse:    { offline: none }
  geo.autocomplete: { offline: none }
  places.search:  { offline: none }
  places.details: { offline: none }
  route.compute:  { offline: none }
  route.matrix:   { offline: none }
terms:
  url: https://cloud.google.com/maps-platform/terms/maps-service-terms
  checked_on: 2026-10-12
  last_modified_seen: 2026-06-10
  commercial_use: allowed
  community_server: false
content:
  display_only_with_renderer: [google]
  headless_allowed: false
  cache:
    latlng_max_days: 30
    ids_cacheable: true
    exceptions:
      - id: geocoding-6.3.2
        scope: end_user_only
        note: "Solo para sostener la función directa al usuario que originó la petición; no sustituye una nueva llamada."
  export_allowed: false
attribution:
  required: true
  non_removable: true
auth: client_restricted_key
egress: [viewport, query_text, coordinates, device_ip]
limits: {}
pricing:
  unit: per_1000_requests
  checked_on: 2026-10-08
  note: "Informativo. Cupos gratuitos y tramos cambian; ver página oficial."
```

```yaml
# contracts/providers/osm-community.manifest.yaml  (ilustrativo)
id: osm-community
plane: client
status: experimental
capabilities:
  map.tiles:      { offline: none }
  geo.forward:    { offline: none }
  geo.reverse:    { offline: none }
terms:
  checked_on: 2026-10-12
  commercial_use: unverified
  community_server: true
forbidden: [geo.autocomplete]
limits: { rate_limit_rps: 1 }
attribution:
  required: true
  text: "© OpenStreetMap contributors"
  non_removable: true
```

## Anexo B — Vectores de reglas de pila (`provider_stack_rules.json`, formato propuesto)

Cada caso declara los proveedores de la pila y el resultado esperado (`ok` o el ID de la regla que falla). Ejemplos:

| Caso | Pila (renderer / tiles / geocoding / routing) | Opciones | Esperado |
|---|---|---|---|
| EXT-V-001 | maplibre / propias / — / — | — | `ok` |
| EXT-V-002 | google / — / google / google | — | `ok` |
| EXT-V-003 | maplibre / propias / google / — | — | `EXT-R-01` |
| EXT-V-004 | maplibre / propias / — / google | — | `EXT-R-01` |
| EXT-V-005 | maplibre / osm-community / — / — | `allowCommunityServers: false` | `EXT-R-02` |
| EXT-V-006 | maplibre / osm-community / — / — | `allowCommunityServers: true`, modo desarrollo | `ok` |
| EXT-V-007 | maplibre / propias / osm-community / — | desarrollo, con autocompletado solicitado | `EXT-R-03` |
| EXT-V-008 | maplibre / osm-community / — / — | desarrollo, paquetes offline activados | `EXT-R-04` |
| EXT-V-009 | (sin renderer) / — / google / — | `headless` sin `legal_review_ref` | `EXT-R-08` |
| EXT-V-010 | maplibre / propias / — / — | manifiesto `stable` con `commercial_use: unverified` | `EXT-R-07` |

## Anexo C — Fuentes consultadas (2026-10-12)

- [Google Maps Platform Service Specific Terms](https://cloud.google.com/maps-platform/terms/maps-service-terms) (última modificación 10-jun-2026): cláusulas de uso con mapas ajenos, caché y atribución.
- [Google Maps Platform pricing](https://developers.google.com/maps/billing-and-pricing/pricing) (actualizada 2026-10-08): SKU, cupos y tramos.
- [Nominatim Usage Policy](https://operations.osmfoundation.org/policies/nominatim/).
- [OSM Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/).
- [OSRM: demo server](https://github.com/Project-OSRM/osrm-backend/wiki/Demo-server).
- [Mapbox Terms of Service](https://www.mapbox.com/legal/tos) (resumen general; **Product Terms no consultados**).
- [`maplibre_gl` en pub.dev](https://pub.dev/packages/maplibre_gl), [`maplibre` en pub.dev](https://pub.dev/packages/maplibre) y [`geolocator` en pub.dev](https://pub.dev/packages/geolocator) (metadatos); licencias leídas en los repositorios [flutter-maplibre-gl](https://github.com/maplibre/flutter-maplibre-gl) y [flutter-geolocator](https://github.com/Baseflow/flutter-geolocator).
- Documento externo "Diseño técnico: servicio universal de geolocalización" v1.0 (aportado por el propietario).

**No consultado o no concluyente en esta ronda:** términos de producto de Mapbox, Azure Maps, HERE, TomTom, MapTiler y Stadia; licencia del paquete `maplibre` (`josxha`); soporte de Places UI Kit desde Flutter; asesoría legal sobre cualquier uso de Google en configuraciones distintas a las descritas.
