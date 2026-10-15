//! JSONPath evaluation for scenario capture and count assertions (contracts/README §5.4).
#![forbid(unsafe_code)]

use serde_json::Value;

/// Evaluates a supported JSONPath expression against a JSON Value.
///
/// Supported expressions:
/// - Simple properties: `$.id`, `$.code`, `$.member.id`
/// - Filter expressions: `$.data[?(@.external_subject_id=='alice')].id`
pub fn extract_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim();
    let path = path
        .strip_prefix("$.")
        .unwrap_or(path.strip_prefix('$').unwrap_or(path));
    if path.is_empty() {
        return Some(root);
    }

    // Check for filter expression: `prefix[?(@.key=='val')].suffix`
    if let Some(bracket_start) = path.find("[?(@.") {
        let array_field = &path[..bracket_start];
        let current = if array_field.is_empty() {
            root
        } else {
            navigate_dot_path(root, array_field)?
        };

        let array = current.as_array()?;
        let rest = &path[bracket_start + 5..]; // after `[?(@.`
        let eq_pos = rest.find("=='")?;
        let key = &rest[..eq_pos];
        let after_eq = &rest[eq_pos + 3..];
        let quote_end = after_eq.find("')]")?;
        let val = &after_eq[..quote_end];

        let after_bracket = &after_eq[quote_end + 3..];
        let suffix = after_bracket.strip_prefix('.').unwrap_or(after_bracket);

        for item in array {
            if let Some(target_val) = item.get(key) {
                let matches = match target_val {
                    Value::String(s) => s == val,
                    Value::Number(n) => n.to_string() == val,
                    _ => false,
                };
                if matches {
                    if suffix.is_empty() {
                        return Some(item);
                    } else {
                        return navigate_dot_path(item, suffix);
                    }
                }
            }
        }
        return None;
    }

    navigate_dot_path(root, path)
}

/// Evaluates the length of an array located at a JSONPath expression.
pub fn count_path(root: &Value, path: &str) -> Option<usize> {
    let val = extract_path(root, path)?;
    val.as_array().map(|a| a.len())
}

fn navigate_dot_path<'a>(mut current: &'a Value, path: &str) -> Option<&'a Value> {
    for part in path.split('.') {
        if part.is_empty() {
            continue;
        }
        current = current.get(part)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_dot_path() {
        let val = json!({
            "id": "ses_123",
            "member": {
                "id": "mem_456"
            }
        });
        assert_eq!(extract_path(&val, "$.id"), Some(&json!("ses_123")));
        assert_eq!(extract_path(&val, "$.member.id"), Some(&json!("mem_456")));
        assert_eq!(extract_path(&val, "member.id"), Some(&json!("mem_456")));
        assert_eq!(extract_path(&val, "$.nonexistent"), None);
    }

    #[test]
    fn test_filter_path() {
        let val = json!({
            "data": [
                { "id": "mem_1", "external_subject_id": "alice" },
                { "id": "mem_2", "external_subject_id": "bob" },
            ]
        });
        assert_eq!(
            extract_path(&val, "$.data[?(@.external_subject_id=='alice')].id"),
            Some(&json!("mem_1"))
        );
        assert_eq!(
            extract_path(&val, "$.data[?(@.external_subject_id=='bob')].id"),
            Some(&json!("mem_2"))
        );
        assert_eq!(
            extract_path(&val, "$.data[?(@.external_subject_id=='charlie')].id"),
            None
        );
    }

    #[test]
    fn test_count_path() {
        let val = json!({
            "items": [1, 2, 3]
        });
        assert_eq!(count_path(&val, "$.items"), Some(3));
        assert_eq!(count_path(&val, "$.missing"), None);
    }
}
