//! Shared OpenAPI loader for the integration suites of `gs-api` (docs/18 §5).
//!
//! Every response (errors included) of every call made by these suites is validated
//! against `contracts/openapi/geostream.v1.yaml`, converted to JSON with `redocly bundle`
//! (docs/19 §6: tests read JSON, no YAML parser).
//!
//! Each integration test binary is its own crate, so not every suite exercises every
//! helper of this module.
#![forbid(unsafe_code)]
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use jsonschema::Validator;
use serde_json::Value;

/// Returns the repository root for this crate.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// The bundled OpenAPI document plus a cache of JSON Schema validators.
pub struct Spec {
    doc: Value,
    /// `operationId` → (`METHOD`, path template).
    ops: BTreeMap<String, (String, String)>,
    validators: Mutex<HashMap<String, Arc<Validator>>>,
}

/// Loads the bundled contract, regenerating the JSON bundle when it is missing or stale.
pub fn load_spec() -> &'static Spec {
    static SPEC: OnceLock<Spec> = OnceLock::new();
    SPEC.get_or_init(|| {
        let root = workspace_root();
        let yaml = root.join("contracts/openapi/geostream.v1.yaml");
        let json_path = std::env::var("GS_OPENAPI_JSON")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("target/openapi/geostream.v1.json"));
        let stale = match (std::fs::metadata(&json_path), std::fs::metadata(&yaml)) {
            (Ok(j), Ok(y)) => j.modified().expect("mtime") < y.modified().expect("mtime"),
            _ => true,
        };
        if stale {
            std::fs::create_dir_all(json_path.parent().expect("parent")).expect("mkdir");
            let status = std::process::Command::new("npx")
                .args(["--yes", "@redocly/cli@2.62.1", "bundle"])
                .arg(&yaml)
                .args(["--ext", "json", "-o"])
                .arg(&json_path)
                .current_dir(&root)
                .status()
                .expect("run `npx @redocly/cli bundle` (requires Node.js)");
            assert!(status.success(), "redocly bundle failed");
        }
        let doc: Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read openapi json"))
                .expect("openapi json");
        let mut ops = BTreeMap::new();
        for (path, item) in doc["paths"].as_object().expect("paths") {
            for (method, op) in item.as_object().expect("path item") {
                if let Some(id) = op.get("operationId").and_then(Value::as_str) {
                    ops.insert(id.to_string(), (method.to_uppercase(), path.clone()));
                }
            }
        }
        Spec {
            doc,
            ops,
            validators: Mutex::new(HashMap::new()),
        }
    })
}

impl Spec {
    /// Returns the operation object of an `operationId`.
    pub fn operation(&self, id: &str) -> &Value {
        let (method, path) = self
            .ops
            .get(id)
            .unwrap_or_else(|| panic!("unknown op {id}"));
        &self.doc["paths"][path][method.to_lowercase()]
    }

    /// Returns every `operationId` of the contract.
    pub fn operation_ids(&self) -> impl Iterator<Item = &String> {
        self.ops.keys()
    }

    /// Returns the `operationId` → (`METHOD`, path template) map of the contract.
    pub fn operations(&self) -> &BTreeMap<String, (String, String)> {
        &self.ops
    }

    /// Follows `$ref` chains inside the document.
    pub fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        let mut current = value;
        while let Some(r) = current.get("$ref").and_then(Value::as_str) {
            let pointer = r.strip_prefix('#').expect("local ref");
            current = self
                .doc
                .pointer(pointer)
                .unwrap_or_else(|| panic!("dangling $ref {r}"));
        }
        current
    }

    fn validator(&self, schema: &Value) -> Arc<Validator> {
        let mut root = schema.clone();
        root.as_object_mut()
            .expect("schema object")
            .insert("components".into(), self.doc["components"].clone());
        let key = serde_json::to_string(schema).expect("schema json");
        let mut cache = self.validators.lock().expect("lock");
        cache
            .entry(key)
            .or_insert_with(|| Arc::new(jsonschema::validator_for(&root).expect("schema")))
            .clone()
    }

    /// Asserts that an instance validates against `schema`.
    pub fn assert_valid(&self, schema: &Value, instance: &Value, what: &str) {
        let validator = self.validator(schema);
        let errors: Vec<String> = validator
            .iter_errors(instance)
            .map(|e| format!("{e} (at {})", e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{what} does not conform to OpenAPI: {errors:?}\nbody: {instance}"
        );
    }

    /// Validates a request body against the schema of its operation.
    pub fn check_request(&self, op_id: &str, body: &Value) {
        let body_spec = self
            .resolve(&self.operation(op_id)["requestBody"])
            .get("content")
            .and_then(|c| c.get("application/json"))
            .and_then(|c| c.get("schema"))
            .unwrap_or_else(|| panic!("{op_id} has no JSON request body"));
        self.assert_valid(body_spec, body, &format!("request of {op_id}"));
    }

    /// Validates a response against the schema of its status code.
    pub fn check_response(&self, op_id: &str, status: u16, content_type: &str, body: &Value) {
        let responses = &self.operation(op_id)["responses"];
        let spec = responses
            .get(status.to_string())
            .or_else(|| responses.get("default"))
            .unwrap_or_else(|| panic!("{op_id}: status {status} is not documented in OpenAPI"));
        let spec = self.resolve(spec);
        let Some(content) = spec.get("content").and_then(Value::as_object) else {
            assert!(
                body.is_null(),
                "{op_id} {status}: body but no content in spec"
            );
            return;
        };
        let (media, media_spec) = content
            .iter()
            .find(|(media, _)| content_type.starts_with(media.as_str()))
            .unwrap_or_else(|| {
                panic!("{op_id} {status}: content-type '{content_type}' not in spec {content:?}")
            });
        let schema = media_spec
            .get("schema")
            .unwrap_or_else(|| panic!("{op_id} {status} {media}: no schema"));
        self.assert_valid(schema, body, &format!("response {status} of {op_id}"));
    }
}
