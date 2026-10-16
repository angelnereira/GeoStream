//! Enum parity between the Protobuf contract, the OpenAPI contract and the docs.
//!
//! Un valor de enum documentado en `docs/` que no existe en `contracts/` es un
//! nombre que nadie puede compilar. Un valor de enum en `contracts/` que no
//! aparece en `docs/` es una capacidad sin explicación. Las dos cosas compilan,
//! las dos pasan una revisión a ojo y las dos son caras.
//!
//! El caso que motivó este módulo: `docs/22` §2.2 documentaba las banderas de
//! plausibilidad como `IMPLAUSIBLE_SPEED`, `IMPLAUSIBLE_ACCELERATION`,
//! `MOCK_REPORTED`, `FILTER_OUTLIER` y `TIME_GAP`, mientras que
//! `location.proto` declaraba `EXCESSIVE_SPEED`, `IMPLAUSIBLE_ACCEL`,
//! `MOCK_FLAGGED`, `OUTLIER_JUMP` y ninguna `TIME_GAP`. Cuatro documentos contra
//! el contrato, y nada en CI lo detectaba.
//!
//! Reglas que verifica:
//!
//! 1. **Proto ↔ OpenAPI.** El enum REST de un nombre de schema coincide con el
//!    enum Protobuf homónimo, en su forma corta sin el prefijo del enum.
//! 2. **Docs → contrato.** Los códigos de la columna indicada de la tabla
//!    indicada existen en ese enum, y no hay valores del enum sin documentar.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Tablas de `docs/` contrastadas con el contrato.
///
/// Cada entrada es (documento, sección de arranque, enum Protobuf, columna de la
/// tabla). Deliberadamente **corta**: una lista exhaustiva de todos los enums
/// del contrato sería otra lista que hay que mantener, y una lista mantenida a
/// medias es peor que una lista corta y justificada.
const DOC_ENUM_TABLES: &[(&str, &str, &str, &str)] = &[
    // (documento, heading que abre la tabla, enum Protobuf, columna)
    (
        "docs/22_TRACKING_ENGINE.md",
        "### 2.2 Puerta de plausibilidad",
        "PlausibilityFlag",
        "Bandera",
    ),
];

fn read(repo_root: &Path, rel: &str) -> Result<String, Vec<String>> {
    let p = repo_root.join(rel);
    match fs::read_to_string(&p) {
        Ok(c) => Ok(c),
        Err(e) => Err(vec![format!("Cannot read {}: {e}", p.display())]),
    }
}

/// Extrae el cuerpo de un `enum X { ... }` del Protobuf.
///
/// Devuelve los nombres con el prefijo eliminado. El prefijo en los `.proto` es
/// la versión del nombre del enum en MAYÚSCULAS (`PlausibilityFlag` ->
/// `PLAUSIBILITY_FLAG_`), no el nombre tal cual; se aceptan las dos formas.
fn proto_enum_body(proto: &str, enum_name: &str) -> Option<Vec<String>> {
    let mut lines = proto.lines().map(str::trim);
    // Buscar la apertura del enum.
    let mut in_enum = false;
    let mut body = Vec::new();
    for line in lines.by_ref() {
        if line.starts_with(&format!("enum {enum_name} ")) {
            in_enum = true;
            continue;
        }
        if !in_enum {
            continue;
        }
        if line.starts_with('}') {
            return Some(body);
        }
        if let Some((name, _)) = line.split_once('=') {
            body.push(name.trim().to_string());
        }
    }
    None
}

/// Convierte `PlausibilityFlag` en el prefijo Protobuf `PLAUSIBILITY_FLAG_`.
///
/// El Protobuf no usa el nombre del enum tal cual: ante `CamelCase` genera
/// `SCREAMING_SNAKE_CASE`. `SessionMode` -> `SESSION_MODE_`,
/// `PlausibilityFlag` -> `PLAUSIBILITY_FLAG_`.
fn screaming_prefix(enum_name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in enum_name.chars().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_uppercase());
    }
    out.push('_');
    out
}

/// Quita el prefijo `<ENUM>_` de un valor Protobuf, si lo tiene.
fn strip_enum_prefix<'a>(enum_name: &str, value: &'a str) -> &'a str {
    let screaming = screaming_prefix(enum_name);
    if let Some(rest) = value.strip_prefix(screaming.as_str()) {
        return rest;
    }
    if let Some(rest) = value.strip_prefix(&format!("{enum_name}_")) {
        return rest;
    }
    value
}

/// Valores de un enum Protobuf, en forma corta y sin `UNSPECIFIED`.
fn proto_enum_values(proto: &str, enum_name: &str) -> Option<BTreeSet<String>> {
    let body = proto_enum_body(proto, enum_name)?;
    Some(
        body.iter()
            .map(|v| strip_enum_prefix(enum_name, v).to_string())
            .filter(|v| !v.is_empty() && v != "UNSPECIFIED")
            .collect(),
    )
}

/// Extrae el bloque de un esquema del OpenAPI por su indentación.
///
/// Los esquemas viven bajo `components.schemas` con 4 espacios de indentación.
/// Un nombre de esquema nuevo empieza en `    Nombre:` y termina en la siguiente
/// línea con esa misma indentación.
fn openapi_schema_block(openapi: &str, schema: &str) -> Option<String> {
    let mut block = String::new();
    let mut inside = false;
    for line in openapi.lines() {
        // Solo líneas de nivel de esquema: exactamente 4 espacios y sin más.
        let is_schema_key = {
            let indent = line.len() - line.trim_start().len();
            indent == 4 && line.trim_end().ends_with(':') && !line.trim().starts_with('#')
        };
        if is_schema_key {
            if inside {
                return Some(block);
            }
            if line.trim() == format!("{schema}:") {
                inside = true;
            }
            continue;
        }
        if inside {
            block.push_str(line);
            block.push('\n');
        }
    }
    if inside { Some(block) } else { None }
}

/// Valores de un `enum:` del OpenAPI, en cualquiera de sus dos formas.
fn openapi_enum_values(openapi: &str, schema: &str) -> Option<BTreeSet<String>> {
    let block = openapi_schema_block(openapi, schema)?;
    if !block.lines().any(|l| l.trim_start().starts_with("enum:")) {
        // El esquema existe pero no es un enum.
        return None;
    }
    let mut values = BTreeSet::new();
    let mut in_enum = false;

    for line in block.lines() {
        let trimmed = line.trim();

        // Forma compacta: `enum: [A, B]`
        if let Some(rest) = trimmed.strip_prefix("enum:") {
            let rest = rest.trim();
            if let Some(list) = rest.strip_prefix('[') {
                for v in list.trim_end_matches(']').split(',') {
                    let v = v.trim().trim_matches('\'');
                    if !v.is_empty() && v != "UNSPECIFIED" {
                        values.insert(v.to_string());
                    }
                }
                return Some(values);
            }
            in_enum = true;
            continue;
        }

        // Forma de bloque: `enum:` seguido de `- A`.
        if in_enum {
            if let Some(v) = trimmed.strip_prefix("- ") {
                let v = v.trim().trim_matches('\'');
                if !v.is_empty() && v != "UNSPECIFIED" {
                    values.insert(v.to_string());
                }
            } else {
                in_enum = false;
            }
        }
    }
    Some(values)
}

/// Localiza el índice de una columna en una tabla Markdown a partir de su
/// encabezado, y devuelve los códigos de DELIMITADOR_CRESTA de esa columna.
///
/// `None` si la tabla o la columna no aparecen. La búsqueda se detiene al salir
/// de la tabla, de modo que dos tablas con la misma columna en el mismo
/// documento no se mezclan.
fn table_column_codes(doc: &str, heading: &str, column: &str) -> Option<BTreeSet<String>> {
    let start = doc.find(heading)? + heading.len();

    let mut in_table = false;
    let mut column_index: Option<usize> = None;
    let mut after_separator = false;
    let mut codes = BTreeSet::new();

    for line in doc[start..].lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            // Una línea que no es de tabla solo es tolerable antes de empezar.
            if in_table {
                break;
            }
            continue;
        }

        let cols: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();

        if !in_table {
            // Todavía no hemos visto el encabezado de la tabla que buscamos.
            if t.contains("---") {
                continue;
            }
            match cols.iter().position(|c| c.eq_ignore_ascii_case(column)) {
                Some(idx) => {
                    column_index = Some(idx);
                    in_table = true;
                }
                None => continue,
            }
            continue;
        }

        // Dentro de la tabla.
        if !after_separator {
            if t.contains("---") {
                after_separator = true;
            }
            continue;
        }

        let Some(idx) = column_index else { break };
        let Some(cell) = cols.get(idx) else { continue };

        // `split` sobre el delimitador produce tramos alternados
        // [prosa, código, prosa, código, …]: los códigos son los de índice impar.
        for (i, part) in cell.split('`').enumerate() {
            if i % 2 == 0 {
                continue;
            }
            let code = part.trim();
            if is_table_code(code) {
                codes.insert(code.to_string());
            }
        }
    }

    if !in_table {
        return None;
    }
    Some(codes)
}

/// ¿Es esto un identificador de enum/constante y no texto de prosa?
fn is_table_code(s: &str) -> bool {
    if s.is_empty() || s.len() > 48 {
        return false;
    }
    let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
    let all_ok = s
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '.');
    has_upper && all_ok && !s.ends_with('.')
}

/// Los enums del contrato que tienen schema REST homónimo.
const CONTRACT_ENUMS: &[(&str, &str)] = &[
    // (fichero .proto, nombre del enum)
    (
        "contracts/proto/geostream/v1/location.proto",
        "PlausibilityFlag",
    ),
    (
        "contracts/proto/geostream/v1/location.proto",
        "EstimateKind",
    ),
    ("contracts/proto/geostream/v1/session.proto", "SessionMode"),
    ("contracts/proto/geostream/v1/session.proto", "HistoryMode"),
    ("contracts/proto/geostream/v1/session.proto", "Availability"),
    (
        "contracts/proto/geostream/v1/common.proto",
        "ConsentPurpose",
    ),
    ("contracts/proto/geostream/v1/common.proto", "ProviderPort"),
    (
        "contracts/proto/geostream/v1/common.proto",
        "SourcePreference",
    ),
];

pub fn check_enum_parity(repo_root: &Path) -> Result<(), Vec<String>> {
    let openapi = read(repo_root, "contracts/openapi/geostream.v1.yaml")?;
    let mut errors = Vec::new();

    // Caché de contenidos .proto.
    let mut protos: std::collections::BTreeMap<&str, String> = Default::default();

    let mut checked_enums = 0usize;
    for (file, enum_name) in CONTRACT_ENUMS {
        let content = match protos.get(*file) {
            Some(c) => c.clone(),
            None => {
                let c = read(repo_root, file)?;
                protos.insert(file, c.clone());
                c
            }
        };

        let Some(proto_values) = proto_enum_values(&content, enum_name) else {
            errors.push(format!("enum {enum_name} not found in {file}"));
            continue;
        };

        // Sin schema REST: no es un enum de la API (p. ej. uno interno del
        // protocolo que solo viaja por el stream). No es un error.
        let Some(api_values) = openapi_enum_values(&openapi, enum_name) else {
            continue;
        };
        if api_values.is_empty() {
            continue;
        }
        checked_enums += 1;

        let missing_in_api: Vec<_> = proto_values.difference(&api_values).collect();
        let missing_in_proto: Vec<_> = api_values.difference(&proto_values).collect();
        if !missing_in_api.is_empty() || !missing_in_proto.is_empty() {
            errors.push(format!(
                "enum `{enum_name}` differs between proto and OpenAPI: \
                 missing in OpenAPI {missing_in_api:?}, missing in proto {missing_in_proto:?}"
            ));
        }
    }

    // Docs -> contrato.
    for (rel, heading, enum_name, column) in DOC_ENUM_TABLES {
        let doc = read(repo_root, rel)?;
        let file = CONTRACT_ENUMS
            .iter()
            .find(|(_, n)| n == enum_name)
            .map(|(f, _)| *f);
        let Some(file) = file else {
            errors.push(format!(
                "DOC_ENUM_TABLES declares unknown enum `{enum_name}` ({rel})"
            ));
            continue;
        };
        let content = match protos.get(file) {
            Some(c) => c.clone(),
            None => {
                let c = read(repo_root, file)?;
                protos.insert(file, c.clone());
                c
            }
        };
        let Some(contract_values) = proto_enum_values(&content, enum_name) else {
            errors.push(format!("enum {enum_name} not found in {file}"));
            continue;
        };

        let Some(documented) = table_column_codes(&doc, heading, column) else {
            errors.push(format!(
                "{rel}: cannot find the `{column}` column of the table under `{heading}`"
            ));
            continue;
        };

        let unknown: Vec<_> = documented.difference(&contract_values).collect();
        if !unknown.is_empty() {
            errors.push(format!(
                "{rel} `{heading}` documents codes that are not values of `{enum_name}`: \
                 {unknown:?}"
            ));
        }
        let undocumented: Vec<_> = contract_values.difference(&documented).collect();
        if !undocumented.is_empty() {
            errors.push(format!(
                "{rel} `{heading}` does not document these `{enum_name}` values: \
                 {undocumented:?}"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: Enum values match across Protobuf, OpenAPI and the normative docs \
         ({} enums proto↔OpenAPI, {} doc tables).",
        checked_enums,
        DOC_ENUM_TABLES.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROTO: &str = "\
enum PlausibilityFlag {
  PLAUSIBILITY_FLAG_UNSPECIFIED = 0;
  PLAUSIBILITY_FLAG_EXCESSIVE_SPEED = 1;
  PLAUSIBILITY_FLAG_OUTLIER_JUMP = 2;
  PLAUSIBILITY_FLAG_LOW_ACCURACY = 3;
}
";

    #[test]
    fn proto_enum_body_is_found_and_prefix_stripped() {
        let values = proto_enum_values(SAMPLE_PROTO, "PlausibilityFlag").unwrap();
        assert_eq!(
            values,
            BTreeSet::from([
                "EXCESSIVE_SPEED".to_string(),
                "OUTLIER_JUMP".to_string(),
                "LOW_ACCURACY".to_string()
            ])
        );
    }

    #[test]
    fn missing_enum_is_none() {
        assert!(proto_enum_values(SAMPLE_PROTO, "NoSuchEnum").is_none());
    }

    const SAMPLE_OPENAPI: &str = "\
components:
  schemas:
    PlausibilityFlag:
      type: string
      enum:
        - EXCESSIVE_SPEED
        - OUTLIER_JUMP
        - LOW_ACCURACY
    EstimateKind:
      type: string
      enum: [FILTERED, MAP_MATCHED]
    Position:
      type: object
      properties:
        latitude: { type: number }
";

    #[test]
    fn openapi_block_enum() {
        let values = openapi_enum_values(SAMPLE_OPENAPI, "PlausibilityFlag").unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.contains("EXCESSIVE_SPEED"));
        // No debe arrastrar valores de otro enum del mismo fichero.
        assert!(!values.contains("FILTERED"));
        assert!(!values.contains("latitude"));
    }

    #[test]
    fn openapi_flow_enum() {
        let values = openapi_enum_values(SAMPLE_OPENAPI, "EstimateKind").unwrap();
        assert_eq!(
            values,
            BTreeSet::from(["FILTERED".to_string(), "MAP_MATCHED".to_string()])
        );
    }

    #[test]
    fn openapi_schema_without_enum_is_none() {
        assert!(openapi_enum_values(SAMPLE_OPENAPI, "Position").is_none());
        assert!(openapi_enum_values(SAMPLE_OPENAPI, "NoSuchSchema").is_none());
    }

    #[test]
    fn table_column_reads_only_the_named_column() {
        let doc = "\
## 2.2 Puerta de plausibilidad

| Bandera | Condición | Otro |
|---|---|---|
| `EXCESSIVE_SPEED` | `v_max` | `BACKGROUND_TRACKING` |
| `LOW_ACCURACY` | `acm` | `REALTIME_TRIP` |

## 2.3 Otra sección

| Bandera | Otra |
|---|---|
| `NO_DEBE_APARECER` | x |
";
        let codes = table_column_codes(doc, "## 2.2 Puerta de plausibilidad", "Bandera").unwrap();
        assert_eq!(
            codes,
            BTreeSet::from(["EXCESSIVE_SPEED".to_string(), "LOW_ACCURACY".to_string()])
        );
        assert!(!codes.contains("BACKGROUND_TRACKING"));
        assert!(!codes.contains("NO_DEBE_APARECER"));
    }
}
