# ADR-0037: Contraseña y acceso en dos pasos en la consola, con sesión que sobrevive a la recarga

- **Estado:** Aceptado
- **Fecha:** 2026-11-02
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-CON-001, FR-CON-010, FR-CON-011, FR-CON-013, FR-PRT-004
- **Modifica a:** ADR-0036 §2 (correo + código sin contraseña) y su consecuencia sobre la sesión en memoria. El resto de ADR-0036 (códigos, sesiones, correo) sigue vigente.
- **Decisión de origen:** DR-20261102-02

## Contexto

Con ADR-0036 la consola era un asistente de una sola vez: correo, código y clave de API. No había una cuenta a la que
volver: la sesión vivía solo en memoria, así que recargar la página obligaba a repetir el código, y no existía nada
parecido a «entrar». El propietario pidió una consola de verdad: **crear la cuenta con una contraseña, iniciar sesión
con correo y contraseña, mantener la sesión, y administrar claves y configuración desde un panel**, conservando el
código por correo «por seguridad».

## Decisión

1. **Contraseña por cuenta.** Se fija al registrarse (junto con el código del correo) y se cambia desde la cuenta.
   Se guarda como hash PBKDF2-HMAC-SHA256 con sal aleatoria y 600 000 iteraciones (`GS_CONSOLE_PASSWORD_ITERATIONS`,
   mínimo 100 000), con el número de iteraciones dentro del propio hash para poder subirlo sin bloquear a nadie (se
   actualiza al iniciar sesión). Política: de 10 a 128 caracteres, sin reglas de clases (NIST SP 800-63B), y se
   rechazan una letra repetida, el propio correo y una lista corta de contraseñas comunes.
2. **Iniciar sesión es en dos pasos.** `POST /v1/console/auth/login` comprueba la contraseña y, solo si es correcta,
   envía un código; `POST /v1/console/auth/login/verify` lo canjea y abre la sesión. El primer paso responde `202`
   igual ante una contraseña incorrecta, una cuenta inexistente, una sin contraseña o una deshabilitada, y gasta el mismo
   trabajo en todos los casos: no revela qué correos están registrados (FR-CON-009).
3. **Dos propósitos de código que no se mezclan.** El código de registro y el de acceso viven en la misma tabla bajo
   claves de búsqueda y HMAC con dominios distintos: no comparten código, enfriamiento ni contador de intentos, y uno
   no sirve para el otro. Así pedir un código de registro no puede bloquear ni sustituir al de acceso.
4. **Recuperar la contraseña es repetir el registro.** El código del correo prueba el control del buzón (como en
   cualquier recuperación por correo) y permite fijar una contraseña nueva en una cuenta existente. Las cuentas
   anteriores a este ADR, que no tienen contraseña, entran por ahí la primera vez.
5. **Cambiar la contraseña** exige la actual y cierra las demás sesiones de la cuenta.
6. **La sesión sobrevive a la recarga con `sessionStorage`.** El portal guarda el token `gcs_…` solo mientras la pestaña
   está abierta y lo valida con `GET /v1/console/me` al arrancar; un 401 lo borra. No hay `localStorage` ni cookies.

## Opciones consideradas

- **Argon2id** (recomendación primera de OWASP). No hay ningún crate de hash de contraseñas en la lista aprobada
  (`19` §6.2) y añadirlo exige un ADR propio. PBKDF2-HMAC-SHA256 se construye con `hmac` y `sha2`, ya aprobados, es
  un estándar (RFC 8018) que OWASP admite con 600 000 iteraciones, y se verifica con vectores independientes. Coste: no
  es *memory-hard*, así que un atacante con GPU lo ataca mejor que a Argon2id. Salida: el formato del hash lleva el
  esquema delante (`pbkdf2-sha256$…`), de modo que añadir `argon2id` es aditivo y se migra en el inicio de sesión.
- **Contraseña sin código**, o **código en cada acción**. Descartadas: la contraseña sola deja la cuenta a un
  relleno de credenciales; el código en cada acción vuelve inusable el panel.
- **Cookie `HttpOnly` de sesión.** Más robusta frente a XSS, pero el proxy del portal elimina `Set-Cookie` a propósito
  y exigiría protección CSRF y reabrir ADR-0035. Se descarta por ahora (ver riesgos).
- **`localStorage`.** Descartada: sobrevive al cierre del navegador y a otras pestañas.

## Consecuencias

\+ Una cuenta a la que volver: iniciar sesión, mantener la sesión y gestionar claves y proyectos desde un panel.
\+ El relleno de credenciales no basta: hace falta también el buzón.
\+ Recuperar el acceso sigue siendo posible sin soporte humano.
− Hay un secreto más que custodiar (hashes). Si se filtra la base de datos, PBKDF2 resiste peor que Argon2id.
− Cada comprobación cuesta CPU a propósito (~0,2 s en producción): el inicio de sesión es un objetivo de agotamiento de
  CPU, acotado por el límite por IP y por 128 caracteres como máximo.
− El inicio de sesión necesita el correo cada vez; depende del proveedor y de su tope diario.
− **Riesgo aceptado:** un XSS en el portal podría leer el token de `sessionStorage`. Lo mitigan la CSP estricta
  (`script-src 'self'`, sin `unsafe-inline` para scripts), la ausencia de scripts de terceros, la caducidad de 12 h, la
  revocación al cerrar sesión o cambiar la contraseña y que la API key no se guarda nunca en el navegador.
− Quien controle el buzón puede recuperar la cuenta sin la contraseña; es la propiedad de toda recuperación por correo.
− No hay bloqueo por cuenta tras N contraseñas erróneas: solo el límite por IP. El segundo factor impide entrar con la
  contraseña adivinada, pero un ataque distribuido puede gastar correos (acotado por el tope diario global).

Cambia: `03` (FR-CON-001, FR-CON-013), `06` §8.2 y §8.6, `07` §9.13, `10` (T-44, T-45), `11` §2.0.1, `17` §2.12, `18`.
Contrato: tres operaciones nuevas y el campo `password` de `verifyConsoleCode` (la consola aún no tenía clientes
externos; el contrato sigue en `1.0.0-draft`).

## Seguridad, privacidad y operación

- La contraseña no se registra, no se devuelve y se borra de memoria tras usarla; una prueba captura todos los logs
  durante registro, inicio de sesión y cambio y comprueba que ni la contraseña ni el hash aparecen (y falla si se
  inyecta un `warn!` con la contraseña).
- Operación: `GS_CONSOLE_PASSWORD_ITERATIONS` (por defecto 600 000). Subirlo no invalida hashes existentes.

## Plan de migración

Aditivo: una columna anulable (`0007_console_passwords.sql`). Las cuentas existentes siguen y fijan su contraseña al
recuperar el acceso. No hay datos que convertir.

## Evidencia y referencias

DR-20261102-02; OWASP Password Storage Cheat Sheet (PBKDF2-HMAC-SHA256, 600 000); NIST SP 800-63B; RFC 8018; RFC 7914 §11.
