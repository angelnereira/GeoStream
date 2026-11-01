//! Estilo del mapa propio (ADR-0034 §6, FR-MAP-007).
//!
//! El mapa del portal y de la app de pruebas no debe depender de los servidores comunitarios de
//! OpenStreetMap, que no admiten uso intensivo en producción (`13` §2), y toda pila con datos de
//! OSM debe mostrar la atribución ODbL. Un estilo que apunta por error a `tile.openstreetmap.org`,
//! o que pierde la atribución, compila y se ve bien: este control lo detecta.
//!
//! Reglas que verifica sobre `deploy/tiles/style.json`:
//!
//! 1. Es un estilo MapLibre válido: `version` 8, `sources`, `glyphs` y `layers`.
//! 2. Ninguna URL del estilo apunta a `openstreetmap.org`, fuera del enlace de atribución.
//! 3. Toda fuente vectorial usa las teselas propias (`/tiles/…`) y lleva la atribución
//!    «© OpenStreetMap contributors» y «© OpenMapTiles».
//! 4. Los glifos salen de las fuentes propias (`/tiles/font/…`).
//! 5. Toda capa referencia una fuente declarada.
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use serde_json::Value;

const STYLE: &str = "deploy/tiles/style.json";
const OWN_TILES_PREFIX: &str = "/tiles/";

/// Comprueba las reglas sobre el estilo ya interpretado.
fn check_style(style: &Value) -> Vec<String> {
    let mut errors = Vec::new();

    if style.get("version") != Some(&Value::from(8)) {
        errors.push("`version` debe ser 8".to_string());
    }
    let Some(sources) = style.get("sources").and_then(Value::as_object) else {
        errors.push("falta `sources`".to_string());
        return errors;
    };

    match style.get("glyphs").and_then(Value::as_str) {
        Some(glyphs) if glyphs.starts_with("/tiles/font/") => {}
        Some(glyphs) => errors.push(format!(
            "`glyphs` debe usar las fuentes propias (/tiles/font/…), no '{glyphs}'"
        )),
        None => errors.push("falta `glyphs`".to_string()),
    }

    for (name, source) in sources {
        if source.get("type").and_then(Value::as_str) != Some("vector") {
            continue;
        }
        let tiles = source
            .get("tiles")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if tiles.is_empty() {
            errors.push(format!("la fuente '{name}' no declara `tiles`"));
        }
        for tile in tiles.iter().filter_map(Value::as_str) {
            if !tile.starts_with(OWN_TILES_PREFIX) {
                errors.push(format!(
                    "la fuente '{name}' usa teselas ajenas '{tile}' (FR-MAP-007: deben empezar por {OWN_TILES_PREFIX})"
                ));
            }
        }
        let attribution = source
            .get("attribution")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for required in ["© OpenStreetMap contributors", "© OpenMapTiles"] {
            if !attribution.contains(required) {
                errors.push(format!(
                    "la atribución de la fuente '{name}' no contiene '{required}' (ODbL, `13` §2)"
                ));
            }
        }
    }

    // Ninguna URL de datos puede ser de openstreetmap.org; solo el enlace de atribución.
    let mut urls = Vec::new();
    collect_strings(style, &mut urls);
    for text in urls {
        if text.contains("tile.openstreetmap.org") || text.contains("nominatim.openstreetmap.org") {
            errors.push(format!(
                "el estilo referencia un servidor comunitario de OpenStreetMap: '{text}' (`13` §2)"
            ));
        }
    }

    if let Some(layers) = style.get("layers").and_then(Value::as_array) {
        for layer in layers {
            if let Some(source) = layer.get("source").and_then(Value::as_str)
                && !sources.contains_key(source)
            {
                errors.push(format!(
                    "la capa '{}' usa la fuente no declarada '{source}'",
                    layer.get("id").and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }
    } else {
        errors.push("falta `layers`".to_string());
    }
    errors
}

/// Recoge todas las cadenas de un valor JSON.
fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

pub fn check_tiles_style(root: &Path) -> Result<(), Vec<String>> {
    let path = root.join(STYLE);
    let text = fs::read_to_string(&path)
        .map_err(|e| vec![format!("No se puede leer {}: {e}", path.display())])?;
    let style: Value =
        serde_json::from_str(&text).map_err(|e| vec![format!("{STYLE}: JSON inválido: {e}")])?;
    let errors = check_style(&style);
    if errors.is_empty() {
        let layers = style["layers"].as_array().map_or(0, Vec::len);
        println!(
            "OK: el estilo del mapa propio usa solo teselas y fuentes propias, con atribución ODbL ({layers} capas)."
        );
        Ok(())
    } else {
        Err(errors
            .into_iter()
            .map(|e| format!("{STYLE}: {e}"))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid() -> Value {
        json!({
            "version": 8,
            "glyphs": "/tiles/font/{fontstack}/{range}",
            "sources": {"geostream": {
                "type": "vector",
                "tiles": ["/tiles/panama/{z}/{x}/{y}"],
                "attribution": "© OpenStreetMap contributors · © OpenMapTiles"
            }},
            "layers": [{"id": "water", "type": "fill", "source": "geostream"}]
        })
    }

    #[test]
    fn test_fr_map_007_a_style_with_own_tiles_and_attribution_passes() {
        // covers: FR-MAP-007
        assert!(check_style(&valid()).is_empty());
    }

    #[test]
    fn test_fr_map_007_community_tile_servers_are_rejected() {
        // covers: FR-MAP-007, 13 §2
        let mut style = valid();
        style["sources"]["geostream"]["tiles"] =
            json!(["https://tile.openstreetmap.org/{z}/{x}/{y}.png"]);
        let errors = check_style(&style);
        assert!(
            errors.iter().any(|e| e.contains("teselas ajenas")),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(|e| e.contains("servidor comunitario")),
            "{errors:?}"
        );
    }

    #[test]
    fn test_fr_map_007_missing_attribution_is_rejected() {
        // covers: FR-MAP-007 (ODbL)
        // (atribución, menciones que faltan)
        for (attribution, missing) in [
            ("", 2),
            ("© OpenMapTiles", 1),
            ("© OpenStreetMap contributors", 1),
        ] {
            let mut style = valid();
            style["sources"]["geostream"]["attribution"] = json!(attribution);
            assert_eq!(check_style(&style).len(), missing, "{attribution:?}");
        }
    }

    #[test]
    fn test_fr_map_007_glyphs_must_come_from_own_fonts() {
        // covers: FR-MAP-007
        let mut style = valid();
        style["glyphs"] = json!("https://fonts.example.com/{fontstack}/{range}.pbf");
        assert_eq!(check_style(&style).len(), 1);
    }

    #[test]
    fn test_layers_must_reference_a_declared_source() {
        let mut style = valid();
        style["layers"][0]["source"] = json!("missing");
        assert_eq!(check_style(&style).len(), 1);
    }
}
