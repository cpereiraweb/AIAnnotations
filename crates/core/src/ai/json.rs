//! First-JSON-object extraction from free text (port of Laravel `JsonObjectExtractor`).

use serde_json::{Map, Value};

/// Finds the first valid JSON object in `text`, which may carry prose, ```json
/// fences or stray braces. Every `{` is tried in order.
pub fn first_object(text: &str) -> Option<Map<String, Value>> {
    let bytes = text.as_bytes();
    let mut offset = 0;
    while let Some(rel) = text[offset..].find('{') {
        let start = offset + rel;
        if let Some(end) = matching_brace(bytes, start)
            && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text[start..=end])
        {
            return Some(map);
        }
        offset = start + 1;
    }
    None
}

/// Position of the `}` closing the `{` at `start`, honoring JSON strings and escapes.
fn matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn extracts_object_from_fenced_text() {
        let text = "Aqui está:\n```json\n{\"a\": 1}\n```\nfim";
        assert_eq!(first_object(text), Some(obj(json!({"a": 1}))));
    }

    #[test]
    fn skips_invalid_braces_before_the_object() {
        let text = "use {x} e então {\"a\": \"}\"}";
        assert_eq!(first_object(text), Some(obj(json!({"a": "}"}))));
    }

    #[test]
    fn ignores_escaped_quotes_inside_strings() {
        let text = r#"{"a": "diz \"}\" aqui"}"#;
        assert_eq!(
            first_object(text),
            Some(obj(json!({"a": "diz \"}\" aqui"})))
        );
    }

    #[test]
    fn array_is_not_an_object() {
        assert_eq!(first_object("[1,2] {\"b\":2}"), Some(obj(json!({"b": 2}))));
        // The inner `{` is a valid object, as in v1.
        assert_eq!(first_object("[{\"x\":1}]"), Some(obj(json!({"x": 1}))));
    }

    #[test]
    fn none_without_object() {
        assert_eq!(first_object("sem json"), None);
        assert_eq!(first_object("{"), None);
    }

    #[test]
    fn handles_multibyte_text_around_the_object() {
        assert_eq!(
            first_object("çã {\"a\":\"é\"} ü"),
            Some(obj(json!({"a": "é"})))
        );
    }
}
