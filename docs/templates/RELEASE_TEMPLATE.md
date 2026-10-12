# Plantilla de liberación (Release Template)

**ID:** GS-REL-TMP · **Referencia:** `docs/19_ENGINEERING_STANDARDS.md` §8, `docs/10_SECURITY_PRIVACY.md` §11

---

## 1. Metadatos de la liberación

| Campo | Valor |
|---|---|
| Componente | `server` \| `contracts` \| `sdk-flutter` \| `map-adapters` |
| Versión a liberar | Ej. `server-v0.1.0-alpha.1` / `sdk-flutter-v0.1.0` |
| Tipo de incremento | Mayor (Major) \| Menor (Minor) \| Parche (Patch) \| Prerelease (`-alpha.N`, `-beta.N`, `-rc.N`) |
| Commit de referencia | `<sha-completo>` |
| Fecha prevista | AAAA-MM-DD |
| Responsable de release | `<nombre/usuario>` |

---

## 2. Validación SemVer y Deprecaciones

- [ ] Si es cambio incompatible: ¿cuenta con aprobación explícita del propietario?
- [ ] Si es cambio de contrato REST o Stream: ¿se respetó la política de evolución (OpenAPI `info.version`, Protobuf `protocol_minor`, regla de /v2 desde M2)?
- [ ] Si hay funcionalidades marcadas como obsoletas: ¿se anunció la deprecación en `CHANGELOG.md` y cabeceras con ≥ 6 meses de antelación?
- [ ] `CHANGELOG.md` actualizado: sección `## [X.Y.Z] — AAAA-MM-DD` creada a partir de `## [Unreleased]`.

---

## 3. Checklist de Seguridad y Privacidad (`10` §11)

- [ ] Modelo de amenazas revisado frente a los cambios del release.
- [ ] Matriz de autorización REST y stream: 100 % verde.
- [ ] Escenarios SCN-002, SCN-004, SCN-010 verdes; plazo NFR-SEC-002 medido.
- [ ] Prueba de captura de logs sin coordenadas ni secretos (`FR-PRV-004`, `NFR-SEC-003`).
- [ ] `/metrics` sin labels prohibidos (`NFR-OBS-002`).
- [ ] Rate limits y límites de tamaño probados.
- [ ] `cargo deny` / `cargo audit` sin advisories de severidad alta abiertas.
- [ ] Licencias de dependencias compatibles con la licencia del proyecto (ADR-0019, ADR-0020).
- [ ] Fuzzing nocturno sin fallos durante la última semana (desde F10).
- [ ] Retención y borrado probados (desde F10).
- [ ] Imagen de contenedor no-root, sin secretos, con SBOM.
- [ ] Runbook de rotación de claves ensayado (desde F12).

---

## 4. Checklist de Pruebas y Rendimiento

- [ ] CI 100 % verde en `main` (etapas 1 a 5 y 7).
- [ ] Pruebas en dispositivos reales completadas y documentadas en `docs/device-tests/` (desde F5, `18` §9).
- [ ] Informe de benchmarks generado y registrado en `docs/benchmarks/` (desde F6, `18` §8).

---

## 5. Procedimiento de Publicación

1. Crear y firmar el tag git:
   ```bash
   git tag -s <componente>-v<X.Y.Z> -m "Release <componente> v<X.Y.Z>"
   git push origin <componente>-v<X.Y.Z>
   ```
2. Generar artefactos de release y binarios:
   - Binario o paquete empaquetado.
   - Generación de SBOM (Software Bill of Materials).
   - Generación de hashes SHA-256 (`sha256sum * > SHA256SUMS`).
3. Publicar Release en GitHub con el extracto correspondiente de `CHANGELOG.md`.
