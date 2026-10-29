//! Small PHP-parity helpers shared by the transcript readers.

use serde_json::Value;

/// PHP `trim()` default set.
pub(crate) const PHP_WS: [char; 6] = [' ', '\t', '\n', '\r', '\0', '\x0B'];

/// Non-empty string value, else `None` (PHP `self::string`).
pub(super) fn string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// PHP `empty()` on a decoded JSON value.
pub(super) fn php_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => !b,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty() || s == "0",
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
    }
}
