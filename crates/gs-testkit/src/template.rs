//! Template substitution for scenario execution (contracts/README §5.4).
#![forbid(unsafe_code)]

use serde_json::Value;
use std::collections::HashMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Context containing variables and actor tokens for template resolution.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Variables captured during scenario steps (e.g. `session_id`, `alice_member_id`).
    pub variables: HashMap<String, String>,
    /// Active Bearer tokens keyed by actor name (e.g. `alice` -> token).
    pub actor_tokens: HashMap<String, String>,
    /// Clock offset in milliseconds between runner and server (measured in hello).
    pub clock_offset_ms: i64,
}

impl TemplateContext {
    /// Resolves all `{{...}}` occurrences within a string.
    pub fn resolve_str(&self, input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut remaining = input;

        while let Some(start_idx) = remaining.find("{{") {
            result.push_str(&remaining[..start_idx]);
            let after_open = &remaining[start_idx + 2..];
            if let Some(end_idx) = after_open.find("}}") {
                let key = after_open[..end_idx].trim();
                let replacement = self.resolve_key(key);
                result.push_str(&replacement);
                remaining = &after_open[end_idx + 2..];
            } else {
                result.push_str("{{");
                remaining = after_open;
            }
        }
        result.push_str(remaining);
        result
    }

    /// Recursively resolves templates across any JSON Value.
    pub fn resolve_val(&self, val: &Value) -> Value {
        match val {
            Value::String(s) => Value::String(self.resolve_str(s)),
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|item| self.resolve_val(item)).collect())
            }
            Value::Object(map) => {
                let mut new_map = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    let new_k = self.resolve_str(k);
                    let new_v = self.resolve_val(v);
                    new_map.insert(new_k, new_v);
                }
                Value::Object(new_map)
            }
            _ => val.clone(),
        }
    }

    fn resolve_key(&self, key: &str) -> String {
        // 1. Time expressions: now, now-1s, now+500ms
        if let Some(time_str) = self.resolve_time_expr(key) {
            return time_str;
        }

        // 2. Actor token expressions: <actor>.token
        if let Some(actor) = key.strip_suffix(".token")
            && let Some(token) = self.actor_tokens.get(actor)
        {
            return token.clone();
        }

        // 3. Captured variables
        if let Some(val) = self.variables.get(key) {
            return val.clone();
        }

        // Unresolved: keep raw {{key}}
        format!("{{{{{key}}}}}")
    }

    fn resolve_time_expr(&self, expr: &str) -> Option<String> {
        let base = OffsetDateTime::now_utc() + time::Duration::milliseconds(self.clock_offset_ms);

        if expr == "now" {
            return Some(format_rfc3339(base));
        }

        let rest = expr.strip_prefix("now")?;
        let (sign, duration_str) = if let Some(r) = rest.strip_prefix('+') {
            (1i64, r)
        } else {
            let r = rest.strip_prefix('-')?;
            (-1i64, r)
        };

        let (num_str, unit) = if let Some(s) = duration_str.strip_suffix("ms") {
            (s, "ms")
        } else {
            let s = duration_str.strip_suffix('s')?;
            (s, "s")
        };

        let num: i64 = num_str.parse().ok()?;
        let duration = match unit {
            "s" => time::Duration::seconds(num * sign),
            "ms" => time::Duration::milliseconds(num * sign),
            _ => return None,
        };

        let target = base + duration;
        Some(format_rfc3339(target))
    }
}

fn format_rfc3339(dt: OffsetDateTime) -> String {
    dt.format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_variable_and_token_replacement() {
        let mut ctx = TemplateContext::default();
        ctx.variables
            .insert("session_id".to_string(), "ses_999".to_string());
        ctx.actor_tokens
            .insert("alice".to_string(), "eyJ_alice_token".to_string());

        let res = ctx.resolve_str("/v1/sessions/{{session_id}}/members");
        assert_eq!(res, "/v1/sessions/ses_999/members");

        let res = ctx.resolve_str("Bearer {{alice.token}}");
        assert_eq!(res, "Bearer eyJ_alice_token");
    }

    #[test]
    fn test_time_expression_replacement() {
        let ctx = TemplateContext::default();
        let now_str = ctx.resolve_str("{{now}}");
        assert!(now_str.contains('T') && now_str.contains('Z'));

        let past_str = ctx.resolve_str("{{now-1s}}");
        assert!(past_str.contains('T') && past_str.contains('Z'));
    }

    #[test]
    fn test_value_replacement() {
        let mut ctx = TemplateContext::default();
        ctx.variables
            .insert("code".to_string(), "INVITE123".to_string());

        let val = json!({
            "code": "{{code}}",
            "nested": ["{{code}}"]
        });

        let resolved = ctx.resolve_val(&val);
        assert_eq!(resolved["code"], "INVITE123");
        assert_eq!(resolved["nested"][0], "INVITE123");
    }
}
