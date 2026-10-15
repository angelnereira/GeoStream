//! Partial matching per contracts/README §5.5.
#![forbid(unsafe_code)]

use serde_json::Value;

/// Checks if `actual` satisfies all constraints specified in `expected`.
///
/// Rules (contracts/README §5.5):
/// - Objects: each key in `expected` must match recursively in `actual`.
/// - Arrays: must match element by element in length and order.
/// - Primitives: must match; 64-bit integers serialized as strings match numbers.
/// - Null: `null` in expected requires `null` or key absence in actual.
/// - Field names: normalized to snake_case so proto3 camelCase and snake_case match.
pub fn partial_match(expected: &Value, actual: &Value) -> bool {
    let norm_expected = normalize_json_keys(expected);
    let norm_actual = normalize_json_keys(actual);
    match_values(&norm_expected, &norm_actual)
}

fn match_values(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(e), Value::Bool(a)) => e == a,
        (Value::Number(e), Value::Number(a)) => e == a,
        (Value::String(e), Value::String(a)) => e == a,
        // String vs Number tolerance (proto3 int64 travels as string in canonical JSON)
        (Value::String(e), Value::Number(a)) => e == &a.to_string(),
        (Value::Number(e), Value::String(a)) => &e.to_string() == a,
        (Value::Array(e_arr), Value::Array(a_arr)) => {
            if e_arr.len() != a_arr.len() {
                return false;
            }
            for (e_item, a_item) in e_arr.iter().zip(a_arr.iter()) {
                if !match_values(e_item, a_item) {
                    return false;
                }
            }
            true
        }
        (Value::Object(e_map), Value::Object(a_map)) => {
            for (k, e_val) in e_map {
                if e_val.is_null() {
                    if let Some(a_val) = a_map.get(k)
                        && !a_val.is_null()
                    {
                        return false;
                    }
                    continue;
                }
                match a_map.get(k) {
                    Some(a_val) => {
                        if !match_values(e_val, a_val) {
                            return false;
                        }
                    }
                    None => {
                        // In canonical proto3 JSON, default values (false, 0, "") are omitted.
                        // If the expected value is the type's default, an absent key is an equivalent match.
                        if e_val == &Value::Bool(false) {
                            continue;
                        }
                        if let Value::Number(num) = e_val
                            && (num.as_i64() == Some(0)
                                || num.as_u64() == Some(0)
                                || num.as_f64() == Some(0.0))
                        {
                            continue;
                        }
                        if e_val == &Value::String(String::new()) {
                            continue;
                        }
                        return false;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// Recursively normalizes all object keys to snake_case.
pub fn normalize_json_keys(val: &Value) -> Value {
    match val {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(to_snake_case(k), normalize_json_keys(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(normalize_json_keys).collect()),
        _ => val.clone(),
    }
}

/// Converts lowerCamelCase or CamelCase string to snake_case.
pub fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_uppercase() {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("memberId"), "member_id");
        assert_eq!(to_snake_case("locationUpdate"), "location_update");
        assert_eq!(to_snake_case("publishAck"), "publish_ack");
        assert_eq!(
            to_snake_case("horizontalAccuracyM"),
            "horizontal_accuracy_m"
        );
        assert_eq!(to_snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn test_partial_match_primitives_and_objects() {
        let actual = json!({
            "sessionId": "ses_123",
            "sharing": true,
            "maxMembers": 10,
            "extraField": "ignored"
        });

        assert!(partial_match(
            &json!({ "session_id": "ses_123", "sharing": true }),
            &actual
        ));
        assert!(!partial_match(
            &json!({ "session_id": "ses_different" }),
            &actual
        ));
    }

    #[test]
    fn test_proto3_int_string_matching() {
        let actual = json!({
            "sequence": 42
        });
        assert!(partial_match(&json!({ "sequence": "42" }), &actual));

        let actual_str = json!({
            "sequence": "42"
        });
        assert!(partial_match(&json!({ "sequence": 42 }), &actual_str));
    }

    #[test]
    fn test_null_matching() {
        let actual_null = json!({ "last_location_at": null });
        let actual_absent = json!({});

        assert!(partial_match(
            &json!({ "last_location_at": null }),
            &actual_null
        ));
        assert!(partial_match(
            &json!({ "last_location_at": null }),
            &actual_absent
        ));
        assert!(!partial_match(
            &json!({ "last_location_at": null }),
            &json!({ "last_location_at": "2026-10-15T00:00:00Z" })
        ));
    }
}
