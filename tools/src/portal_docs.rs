//! Documentación publicada por el portal (ADR-0034 §2, FR-PRT-001, FR-PRT-003).
//!
//! El portal publica los contratos y las guías para integradores. Publicar por error el
//! modelo de amenazas, las operaciones internas o la configuración sería una fuga de
//! información que nadie ve en una revisión de PR: el manifiesto es una lista explícita y
//! este control falla ante cualquier documento que no esté en la lista permitida.
//!
//! Reglas que verifica:
//!
//! 1. Cada línea del manifiesto tiene un tipo conocido (`openapi`, `proto`, `doc`) y una ruta.
//! 2. Todo `doc` pertenece a la lista permitida; ninguno de la lista prohibida.
//! 3. Todo archivo del manifiesto existe.
//! 4. **Completitud de contratos:** el OpenAPI y cada `.proto` de `contracts/proto/geostream/v1/`
//!    están en el manifiesto, de modo que los publicados son exactamente los del repositorio.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Documentos que pueden publicarse (ADR-0034 §2).
const ALLOWED_DOCS: &[&str] = &[
    "docs/07_REST_API.md",
    "docs/08_STREAM_PROTOCOL.md",
    "docs/09_VALIDATION_ERRORS.md",
    "docs/12_FLUTTER_SDK.md",
    "docs/13_MAPS_PROVIDERS.md",
    "docs/15_INTEGRATION_GUIDE.md",
];

/// Rutas que no se publican nunca, aunque algún día entren en la lista permitida por error.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "docs/10_",
    "docs/16_",
    "docs/17_",
    "docs/decisions/",
    "docs/adr/",
    "docs/proposals/",
    "docs/PROGRESS",
    "docs/TRACEABILITY",
];

const MANIFEST: &str = "apps/portal/docs-manifest.txt";
const OPENAPI: &str = "contracts/openapi/geostream.v1.yaml";
const PROTO_DIR: &str = "contracts/proto/geostream/v1";

/// Una línea válida del manifiesto.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: String,
    path: String,
}

/// Interpreta el manifiesto; devuelve las entradas y los errores de formato.
fn parse_manifest(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            [kind @ ("openapi" | "proto" | "doc"), path] => entries.push(Entry {
                kind: (*kind).to_string(),
                path: (*path).to_string(),
            }),
            _ => errors.push(format!(
                "{MANIFEST}:{}: línea inválida '{line}' (se espera '<openapi|proto|doc> <ruta>')",
                index + 1
            )),
        }
    }
    (entries, errors)
}

/// Comprueba las reglas 2 y 4 sobre las entradas ya interpretadas.
fn check_entries(entries: &[Entry], contract_files: &BTreeSet<String>) -> Vec<String> {
    let mut errors = Vec::new();
    for entry in entries.iter().filter(|e| e.kind == "doc") {
        if let Some(prefix) = FORBIDDEN_PREFIXES
            .iter()
            .find(|prefix| entry.path.starts_with(**prefix))
        {
            errors.push(format!(
                "{}: no se publica nunca (prefijo prohibido '{prefix}', 10 T-40)",
                entry.path
            ));
        } else if !ALLOWED_DOCS.contains(&entry.path.as_str()) {
            errors.push(format!(
                "{}: no está en la lista de documentos publicables (ADR-0034 §2)",
                entry.path
            ));
        }
    }
    let listed: BTreeSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    for contract in contract_files {
        if !listed.contains(contract.as_str()) {
            errors.push(format!(
                "{contract}: es un contrato del repositorio y falta en el manifiesto (FR-PRT-001)"
            ));
        }
    }
    errors
}

/// Contratos que deben publicarse: el OpenAPI y cada `.proto` de la versión 1.
fn contract_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    files.insert(OPENAPI.to_string());
    let dir = root.join(PROTO_DIR);
    let read =
        fs::read_dir(&dir).map_err(|e| format!("No se puede leer {}: {e}", dir.display()))?;
    for entry in read {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".proto") {
            files.insert(format!("{PROTO_DIR}/{name}"));
        }
    }
    Ok(files)
}

pub fn check_portal_docs(root: &Path) -> Result<(), Vec<String>> {
    let manifest_path = root.join(MANIFEST);
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| vec![format!("No se puede leer {}: {e}", manifest_path.display())])?;
    let (entries, mut errors) = parse_manifest(&text);

    let contracts = contract_files(root).map_err(|e| vec![e])?;
    errors.extend(check_entries(&entries, &contracts));
    for entry in &entries {
        if !root.join(&entry.path).is_file() {
            errors.push(format!("{}: no existe", entry.path));
        }
    }

    if errors.is_empty() {
        println!(
            "OK: el portal publica {} archivos del manifiesto, todos permitidos y con todos los contratos ({} contratos).",
            entries.len(),
            contracts.len()
        );
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, path: &str) -> Entry {
        Entry {
            kind: kind.to_string(),
            path: path.to_string(),
        }
    }

    fn contracts() -> BTreeSet<String> {
        [OPENAPI, "contracts/proto/geostream/v1/stream.proto"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    fn complete() -> Vec<Entry> {
        vec![
            entry("openapi", OPENAPI),
            entry("proto", "contracts/proto/geostream/v1/stream.proto"),
        ]
    }

    #[test]
    fn test_fr_prt_003_a_complete_allowed_manifest_passes() {
        // covers: FR-PRT-003, FR-PRT-001
        let mut entries = complete();
        entries.push(entry("doc", "docs/15_INTEGRATION_GUIDE.md"));
        assert!(check_entries(&entries, &contracts()).is_empty());
    }

    #[test]
    fn test_fr_prt_003_security_and_operations_docs_are_never_published() {
        // covers: FR-PRT-003, 10 T-40
        for path in [
            "docs/10_SECURITY_PRIVACY.md",
            "docs/16_OPERATIONS.md",
            "docs/17_CONFIGURATION.md",
            "docs/decisions/accepted/DR-20261101-01.md",
            "docs/adr/ADR-0033-developer-console-and-gravital-oidc.md",
            "docs/PROGRESS.md",
        ] {
            let mut entries = complete();
            entries.push(entry("doc", path));
            let errors = check_entries(&entries, &contracts());
            assert_eq!(errors.len(), 1, "{path}: {errors:?}");
            assert!(errors[0].contains("nunca"), "{path}: {errors:?}");
        }
    }

    #[test]
    fn test_fr_prt_003_a_doc_outside_the_allowed_list_is_rejected() {
        // covers: FR-PRT-003
        let mut entries = complete();
        entries.push(entry("doc", "docs/05_DOMAIN_MODEL.md"));
        let errors = check_entries(&entries, &contracts());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("lista de documentos publicables"));
    }

    #[test]
    fn test_fr_prt_001_a_contract_missing_from_the_manifest_is_reported() {
        // covers: FR-PRT-001
        let entries = vec![entry("openapi", OPENAPI)];
        let errors = check_entries(&entries, &contracts());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("stream.proto"));
    }

    #[test]
    fn test_manifest_lines_with_unknown_kinds_or_extra_fields_are_errors() {
        let (entries, errors) = parse_manifest(
            "# comentario\n\nopenapi a.yaml\nsecret docs/10_SECURITY_PRIVACY.md\ndoc a b\n",
        );
        assert_eq!(entries, vec![entry("openapi", "a.yaml")]);
        assert_eq!(errors.len(), 2);
    }
}
