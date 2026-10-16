//! Phase, scope and ID-prefix consistency between contracts and the normative docs.
//!
//! Cubre los tres controles que la pista E añade a `docs/18` §10.1:
//!
//! 1. `x-gs-phase` de OpenAPI corresponde a una fase declarada en `21_ROADMAP.md`.
//! 2. Todo scope usado en `x-gs-auth` existe en las tablas de `06_IDENTITY_AUTH.md`
//!    (§3.4 para aplicación, §6 para dispositivo). Un scope que solo existe en el
//!    contrato es una capacidad que nadie ha revisado ni documentado.
//! 3. Los prefijos de ID (`05` §2) coinciden con `contracts/test-vectors/ids.json`
//!    y con los patrones `pattern:` del OpenAPI.
//!
//! Por qué importa: las tres clases de error son invisibles en la revisión de un
//! PR. Un `x-gs-phase: E4` en una operación que pertenece a E5 no rompe nada
//! visible; solo rompe la regla de «una capacidad nueva no se activa por
//! accidente», que es la que sostiene la promesa de aditividad de la pista E.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Fases que la línea base declara. `E0`..`E9` se declaran en `21_ROADMAP.md`.
const BASELINE_PHASES: &[&str] = &[
    "F0", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
    "F15", "F16",
];

fn read(repo_root: &Path, rel: &str) -> Result<String, Vec<String>> {
    let p = repo_root.join(rel);
    match fs::read_to_string(&p) {
        Ok(c) => Ok(c),
        Err(e) => Err(vec![format!("Cannot read {}: {e}", p.display())]),
    }
}

/// Extrae los identificadores de fase `## F<NN> —` y `### E<N> —` del roadmap.
fn parse_roadmap_phases(roadmap: &str) -> BTreeSet<String> {
    let mut phases = BTreeSet::new();
    for base in BASELINE_PHASES {
        phases.insert((*base).to_string());
    }
    for line in roadmap.lines() {
        let trimmed = line.trim_start();
        // Encabezado de sección del roadmap: `## F4 — ...`, `### E2 — ...`.
        let rest = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "));
        if let Some(rest) = rest {
            let head: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if head.starts_with('E') && head.len() > 1 {
                let digits = &head[1..];
                if digits.chars().all(|c| c.is_ascii_digit()) {
                    phases.insert(head);
                }
            }
        }
    }
    phases
}

/// Extrae los valores de `x-gs-phase` y de `x-gs-auth` de cada operación.
struct OpInfo {
    operation_id: String,
    phase: Option<String>,
    app_scope: Option<String>,
    device_scope: Option<String>,
}

fn parse_openapi_operations(openapi: &str) -> Vec<OpInfo> {
    let mut ops = Vec::new();
    let mut current: Option<OpInfo> = None;

    for line in openapi.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("operationId:") {
            if let Some(prev) = current.take() {
                ops.push(prev);
            }
            current = Some(OpInfo {
                operation_id: rest.trim().to_string(),
                phase: None,
                app_scope: None,
                device_scope: None,
            });
            continue;
        }
        let Some(op) = current.as_mut() else { continue };

        if let Some(rest) = trimmed.strip_prefix("x-gs-phase:") {
            op.phase = Some(rest.trim().to_string());
        }
        if let Some(auth_pos) = line.find("x-gs-auth:") {
            let auth_str = line[auth_pos + "x-gs-auth:".len()..].trim();
            if let (Some('{'), Some('}')) = (auth_str.chars().next(), auth_str.chars().last()) {
                let inner = &auth_str[1..auth_str.len() - 1];
                for part in inner.split(',') {
                    let part = part.trim();
                    if let Some((k, v)) = part.split_once(':') {
                        let k = k.trim();
                        let v = v.trim().trim_matches('\'').trim_matches('"');
                        match k {
                            "application" => op.app_scope = Some(v.to_string()),
                            "device" => op.device_scope = Some(v.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if let Some(last) = current {
        ops.push(last);
    }
    ops
}

/// Extrae los scopes de las tablas `06_IDENTITY_AUTH.md` §3.4 (aplicación) y §6
/// (dispositivo). Los valores quite-delimitados de la columna "Permite".
fn parse_documented_scopes(auth_doc: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut app = BTreeSet::new();
    let mut device = BTreeSet::new();

    let mut section = "";
    let mut in_table = false;

    for line in auth_doc.lines() {
        if line.starts_with("### 3.4 ") {
            section = "app";
            in_table = false;
            continue;
        }
        if line.starts_with("### 3.5") || line.starts_with("## 4.") {
            section = "";
            in_table = false;
            continue;
        }
        if line.starts_with("## 5.") || line.starts_with("## 7.") {
            section = "";
            in_table = false;
            continue;
        }
        if line.starts_with("## 6. ") {
            section = "device";
            in_table = false;
            continue;
        }
        if section.is_empty() {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.contains("---") {
            in_table = true;
            continue;
        }
        if !trimmed.starts_with('|') {
            in_table = false;
            continue;
        }
        if !in_table || trimmed.contains("Scope |") {
            continue;
        }

        let cols: Vec<_> = trimmed.split('|').map(str::trim).collect();
        // La primera columna es el scope entre acentos graves.
        let Some(first) = cols.get(1) else { continue };
        if let Some(start) = first.find('`')
            && let Some(end) = first[start + 1..].find('`')
        {
            let scope = first[start + 1..start + 1 + end].to_string();
            match section {
                "app" => {
                    app.insert(scope);
                }
                "device" => {
                    device.insert(scope);
                }
                _ => {}
            }
        }
    }
    (app, device)
}

/// Prefijos de ID declarados en `05_DOMAIN_MODEL.md` §2.
fn parse_domain_prefixes(domain_doc: &str) -> BTreeSet<String> {
    let mut prefixes = BTreeSet::new();
    if let Some(start) = domain_doc.find("Regex general") {
        let rest = &domain_doc[start..];
        if let Some(open) = rest.find("^(") {
            let inner = &rest[open + 2..];
            if let Some(close) = inner.find(')') {
                for p in inner[..close].split('|') {
                    let p = p.trim();
                    if !p.is_empty() {
                        prefixes.insert(p.to_string());
                    }
                }
            }
        }
    }
    prefixes
}

/// Patrones `{type, pattern}` de los esquemas de ID del OpenAPI.
///
/// Solo se recogen los esquemas cuyo patrón tiene **exactamente** la forma pública
/// de `05` §2 (`<prefijo>_[0-9a-f]{32}$`). Ese filtro descarta a propósito
/// `ExternalSubjectId`, `installation_id`, `StreamTicket` y `RequestId`, cuyos
/// formatos son otros, y descarta también cualquier `pattern:` que se haya
/// asociado por error al esquema anterior.
fn parse_openapi_id_patterns(openapi: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in openapi.lines() {
        let trimmed = line.trim();

        // Formato compacto: `TagId: { type: string, pattern: '^tag_[0-9a-f]{32}$' }`
        if let Some((name, rest)) = trimmed.split_once(": {")
            && name.ends_with("Id")
        {
            current = Some(name.to_string());
            if let Some(pat) = extract_pattern(rest)
                && is_public_id_pattern(&pat)
            {
                out.insert(name.to_string(), pat);
            }
            continue;
        }

        // Formato expandido:
        //   TagId:
        //     type: string
        //     pattern: '^tag_[0-9a-f]{32}$'
        if let Some(name) = trimmed.strip_suffix(':') {
            if name.ends_with("Id") {
                current = Some(name.to_string());
            }
            continue;
        }

        if let Some(name) = current.as_ref()
            && let Some(rest) = trimmed.strip_prefix("pattern: ")
        {
            let pat = rest.trim().trim_matches('\'').to_string();
            if is_public_id_pattern(&pat) {
                out.insert(name.clone(), pat);
            }
        }
    }
    out
}

/// ¿El patrón tiene la forma pública de `05` §2: `^<prefijo>_[0-9a-f]{32}$`?
fn is_public_id_pattern(pattern: &str) -> bool {
    let Some(body) = pattern.strip_prefix('^') else {
        return false;
    };
    let Some(stripped) = body.strip_suffix("$") else {
        return false;
    };
    let Some((prefix, hex)) = stripped.split_once("_") else {
        return false;
    };
    !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_lowercase()) && hex == "[0-9a-f]{32}"
}

/// ¿El valor tiene la forma pública `<prefijo>_<32 hex>` de `05` §2?
fn is_public_id_value(value: &str) -> bool {
    let Some((prefix, hex)) = value.split_once('_') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_lowercase())
        && hex.len() == 32
        && hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn extract_pattern(s: &str) -> Option<String> {
    let pos = s.find("pattern: ")?;
    let rest = s[pos + "pattern: ".len()..].trim();
    let rest = rest.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Prefijos que aparecen en un patrón `'^<prefijo>_[0-9a-f]{32}$'`.
fn prefix_of_pattern(pattern: &str) -> Option<String> {
    let body = pattern.strip_prefix('^')?;
    let (prefix, _) = body.split_once('_')?;
    Some(prefix.to_string())
}

pub fn check_phases_and_scopes(repo_root: &Path) -> Result<(), Vec<String>> {
    let roadmap = read(repo_root, "docs/21_ROADMAP.md")?;
    let auth_doc = read(repo_root, "docs/06_IDENTITY_AUTH.md")?;
    let domain_doc = read(repo_root, "docs/05_DOMAIN_MODEL.md")?;
    let openapi = read(repo_root, "contracts/openapi/geostream.v1.yaml")?;
    let ids_vectors = read(repo_root, "contracts/test-vectors/ids.json")?;

    let phases = parse_roadmap_phases(&roadmap);
    let (app_scopes, device_scopes) = parse_documented_scopes(&auth_doc);
    let domain_prefixes = parse_domain_prefixes(&domain_doc);
    let ops = parse_openapi_operations(&openapi);

    let mut errors = Vec::new();

    // 1. Fases declaradas
    if !phases.iter().any(|p| p.starts_with('E')) {
        errors.push(
            "docs/21_ROADMAP.md does not declare any E<n> phase; the E track roadmap is missing"
                .to_string(),
        );
    }

    // 2. x-gs-phase y scopes
    for op in &ops {
        match &op.phase {
            Some(phase) if phases.contains(phase) => {}
            Some(phase) => errors.push(format!(
                "Operation `{}` has x-gs-phase `{}` which is not declared in 21_ROADMAP.md",
                op.operation_id, phase
            )),
            None => errors.push(format!(
                "Operation `{}` has no x-gs-phase; every operation must declare one",
                op.operation_id
            )),
        }

        for (kind, scope) in [("application", &op.app_scope), ("device", &op.device_scope)] {
            let Some(scope) = scope else { continue };
            match scope.as_str() {
                "none" | "forbidden" => continue,
                _ => {}
            }
            let documented = if kind == "application" {
                &app_scopes
            } else {
                &device_scopes
            };
            if !documented.contains(scope) {
                errors.push(format!(
                    "Operation `{}` requires scope `{}` for {}, which is not documented in \
                     06_IDENTITY_AUTH.md §{}",
                    op.operation_id,
                    scope,
                    kind,
                    if kind == "application" { "3.4" } else { "6" }
                ));
            }
        }
    }

    // 3. Prefijos de ID: docs/05 vs OpenAPI vs vectores
    let openapi_patterns = parse_openapi_id_patterns(&openapi);
    for (name, pattern) in &openapi_patterns {
        let Some(prefix) = prefix_of_pattern(pattern) else {
            errors.push(format!(
                "OpenAPI schema `{name}` has pattern `{pattern}` that does not start with `^<prefix>_...`"
            ));
            continue;
        };
        if !domain_prefixes.contains(&prefix) {
            errors.push(format!(
                "OpenAPI schema `{name}` uses prefix `{prefix}_` which is not declared in \
                 05_DOMAIN_MODEL.md §2 (declared: {:?})",
                domain_prefixes.iter().collect::<Vec<_>>()
            ));
        }
    }

    let ids_json: serde_json::Value = serde_json::from_str(&ids_vectors)
        .map_err(|e| vec![format!("ids.json is not valid JSON: {e}")])?;
    for entry in ids_json
        .get("valid")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
    {
        let (Some(value), Some(kind)) = (
            entry.get("value").and_then(|v| v.as_str()),
            entry.get("type").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        // Solo los valores con la forma pública `<prefijo>_<32 hex>` declaran un
        // prefijo de entidad; `external_subject_id` e `installation_id` no.
        if !is_public_id_value(value) {
            continue;
        }
        let Some(prefix) = value.split('_').next() else {
            continue;
        };
        if !domain_prefixes.contains(prefix) {
            errors.push(format!(
                "ids.json declares a valid `{kind}` with prefix `{prefix}_` which is not declared \
                 in 05_DOMAIN_MODEL.md §2"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: All {} operations declare a known x-gs-phase, use documented scopes, and ID prefixes \
         match 05 §2 ({} phases, {} app scopes, {} device scopes, {} ID schemas).",
        ops.len(),
        phases.len(),
        app_scopes.len(),
        device_scopes.len(),
        openapi_patterns.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roadmap_phases() {
        let roadmap = "## F4 — Stream\n### E2 — Motor\n### E0 — Documentación\n### E1 — Núcleo\n";
        let phases = parse_roadmap_phases(roadmap);
        assert!(phases.contains("F4"));
        assert!(phases.contains("E0"));
        assert!(phases.contains("E2"));
        assert!(!phases.contains("E99"));
    }

    #[test]
    fn recognises_public_id_values() {
        assert!(is_public_id_value("tag_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a"));
        assert!(!is_public_id_value("u-48213"));
        assert!(!is_public_id_value("3f1c2a7e-9b0d-4c1e-8f2a-6d5b4c3a2e10"));
        assert!(!is_public_id_value("ses_0192B6C0A1E47C3F9D2A7B5E8F1C0D3A"));
    }

    #[test]
    fn filters_non_id_patterns() {
        assert!(is_public_id_pattern("^tag_[0-9a-f]{32}$"));
        assert!(!is_public_id_pattern("^[A-Za-z0-9._:\\-]{1,128}$"));
        assert!(!is_public_id_pattern("^https://"));
    }

    #[test]
    fn extracts_prefix_of_pattern() {
        assert_eq!(
            prefix_of_pattern("^tag_[0-9a-f]{32}$").as_deref(),
            Some("tag")
        );
        assert_eq!(prefix_of_pattern("^[0-9a-f]{32}$"), None);
    }

    #[test]
    fn extracts_pattern_from_flow_mapping() {
        assert_eq!(
            extract_pattern(" type: string, pattern: '^trp_[0-9a-f]{32}$' }").as_deref(),
            Some("^trp_[0-9a-f]{32}$")
        );
    }

    #[test]
    fn parses_openapi_operations_with_auth() {
        let yaml = "  /v1/x:\n    post:\n      operationId: doThing\n      x-gs-phase: E4\n      x-gs-auth: { application: 'pools:manage', device: forbidden }\n";
        let ops = parse_openapi_operations(yaml);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation_id, "doThing");
        assert_eq!(ops[0].phase.as_deref(), Some("E4"));
        assert_eq!(ops[0].app_scope.as_deref(), Some("pools:manage"));
        assert_eq!(ops[0].device_scope.as_deref(), Some("forbidden"));
    }
}
