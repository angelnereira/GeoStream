# ADR-0036: Cuentas propias de la consola con correo y código de un solo uso

- **Estado:** Aceptado
- **Fecha:** 2026-11-01
- **Responsable:** propietario del proyecto
- **Requisitos afectados:** FR-CON-001, FR-CON-002, FR-CON-009 a FR-CON-012, FR-PRT-004
- **Sustituye a:** ADR-0033 §2 (inicio de sesión por OIDC). El resto de ADR-0033 (§3 modelo de datos, §4 cuotas) sigue vigente.
- **Decisión de origen:** DR-20261101-02

## Contexto

La consola necesita registro e inicio de sesión ya. ADR-0033 los ataba a que la identidad de Gravital fuera proveedor
OIDC, y esa identidad es un servicio gRPC interno con mTLS y secreto simétrico (DR-20261101-02): no lo es hoy ni lo
será pronto. El propietario eligió cuentas propias ahora y «Entrar con Gravital» después.

## Opciones consideradas

1. **Correo + código de un solo uso, sin contraseña.** Una sola ceremonia para registrarse e iniciar sesión; no hay
   contraseñas que filtrar, rotar ni verificar contra listas de claves comprometidas.
2. **Correo + contraseña.** Obliga a guardar hashes, políticas de contraseña, recuperación y bloqueo.
3. **Passkeys (WebAuthn).** La mejor protección frente al phishing, pero exige una librería no aprobada y soporte de
   plataforma que no se ha medido; queda como mejora posterior.

## Decisión

### 1. Cuenta
`console_accounts` (`acc_…`): correo normalizado (minúsculas, sin alias de `+` solo si el proveedor lo declara; por
defecto se conserva tal cual), `email_verified_at`, estado (`ACTIVE`, `DISABLED`), fechas. Sin contraseña. Una tabla
`console_identities(account_id, issuer, subject)` queda reservada para federar «Entrar con Gravital» sin cambiar la cuenta.

### 2. Registro e inicio de sesión (una sola ceremonia)
1. `POST /v1/console/auth/code` con `{email}`: responde **siempre** 202 con el mismo cuerpo, exista o no la cuenta
   (sin enumeración de cuentas) y envía un código a ese correo.
2. `POST /v1/console/auth/verify` con `{email, code}`: si el código es válido y no ha caducado, crea la cuenta si no
   existía (el alta exige `accept_terms: true`) y devuelve un token de sesión.
- El código tiene `GS_CONSOLE_OTP_LENGTH` dígitos, se genera con el generador criptográfico del sistema, se guarda solo
  como HMAC-SHA256 con una clave del servidor, caduca a los `GS_CONSOLE_OTP_TTL_S` segundos, es de un solo uso y admite
  `GS_CONSOLE_OTP_MAX_ATTEMPTS` intentos; después se invalida. La comparación es de tiempo constante.
- Pedir otro código para el mismo correo antes de `GS_CONSOLE_OTP_COOLDOWN_S` no envía correo y responde igual.
- Límites: `GS_CONSOLE_SIGNUP_RPM_PER_IP` por IP, y un tope diario global de correos (`GS_CONSOLE_EMAIL_DAILY_CAP`)
  para que nadie convierta el servicio en un coste por correo.

### 3. Sesión
Token opaco de 256 bits (`gcs_…`), guardado solo como SHA-256, con caducidad absoluta `GS_CONSOLE_SESSION_TTL_S`.
Viaja en `Authorization: Bearer`; no hay cookies, así que no hay CSRF que defender. El portal lo guarda **solo en
memoria**: al recargar la página se vuelve a pedir un código. Cerrar sesión lo revoca en el servidor.

### 4. Correo
Un puerto `EmailSender` con dos adaptadores: `resend` (HTTPS, con `GS_CONSOLE_EMAIL_API_KEY`) y `file` (solo `dev` y
`test`: escribe cada mensaje en un directorio, nunca en el log). El código y el correo en claro **nunca** se escriben
en logs, métricas ni trazas (`10` §8.2); en los logs el correo solo aparece como hash truncado.

### 5. Qué puede hacer una cuenta
Lo de ADR-0033 §3: organizaciones (tenants) y proyectos (aplicaciones) con rol `OWNER`, `ADMIN` o `VIEWER`; crear,
rotar y revocar API keys con el código existente de `gs-service::admin` (la misma ruta que la CLI), con la cuenta como
actor de auditoría. La key en claro se muestra una sola vez.

### 6. «Entrar con Gravital» (posterior)
Cuando `identity` publique OIDC con claves asimétricas (T-F15-02), se añade como segundo método (T-F15-20) y enlaza con
la cuenta por correo verificado mediante `console_identities`.

## Consecuencias

\+ La consola funciona ya y no depende de cambios en la plataforma de pagos.
\+ Sin contraseñas que custodiar.
− Dos sistemas de cuentas hasta que se federe (acotado por `console_identities`).
− Depende de un proveedor de correo y de su entregabilidad; el tope diario protege el coste.
− Una sesión que vive solo en memoria obliga a pedir un código tras cada recarga (se puede mitigar después con una
cookie de refresco `HttpOnly`, con su propio ADR).

Cambia: `03`, `10`, `17`, `21`, `docs/adr/README.md`. Contratos: en T-F15-03 (un único PR sobre `contracts/`).

## Seguridad, privacidad y operación

- Amenazas T-41 a T-43 en `10`: fuerza bruta del código, enumeración de cuentas y abuso del correo como coste.
- El correo es un dato personal: se inventaría en `10` §6.2, se borra con la cuenta y no se comparte.
- Operación: una clave de proveedor de correo como secreto de Railway y la clave HMAC de los códigos.

## Plan de migración

Aditivo. La CLI `geostream admin` sigue funcionando. Las organizaciones existentes se vinculan a una cuenta con un
comando de administración (T-F15-16).

## Evidencia y referencias

DR-20261101-02; revisión del código de `identity` de `Plenty-Pay` el 2026-11-01; OWASP ASVS V2 (autenticación) y V3 (sesiones).
