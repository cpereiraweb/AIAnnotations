use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::ControlFlow;
use std::path::Path;

use serde_json::Value;

/// Calls `f` for every JSON object/array line of a JSONL file. Empty, non-UTF-8,
/// invalid or truncated lines are skipped (e.g. the last line of a transcript being written).
/// Reads bytes, so a corrupted line never aborts the rest of the file.
pub fn for_each_value(path: &Path, mut f: impl FnMut(Value)) -> std::io::Result<()> {
    for_each_value_until(path, |v| {
        f(v);
        ControlFlow::Continue(())
    })
}

/// Like `for_each_value`, but stops reading as soon as `f` returns `Break` (no further line is
/// read or parsed).
pub fn for_each_value_until(
    path: &Path,
    mut f: impl FnMut(Value) -> ControlFlow<()>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buf = Vec::new();

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        if buf.trim_ascii().is_empty() {
            continue;
        }
        if let Ok(value @ (Value::Object(_) | Value::Array(_))) = serde_json::from_slice(&buf)
            && f(value).is_break()
        {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review Focus 2: bytes inválidos, linha truncada e linha gigante não derrubam a leitura.
    #[test]
    fn jsonl_skips_invalid_utf8_and_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut data = b"{\"a\":1}\n".to_vec();
        data.extend_from_slice(b"{\"b\":\"\xFF\xFE\"}\n");
        data.extend_from_slice(b"{\"a\":\n");
        data.extend(std::iter::repeat_n(b'x', 2 * 1024 * 1024));
        data.push(b'\n');
        data.extend_from_slice(b"\n{\"a\":2}");
        std::fs::write(&path, data).unwrap();

        let mut seen = Vec::new();
        for_each_value(&path, |v| seen.push(v)).unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1]["a"], 2);
    }

    #[test]
    fn jsonl_until_stops_calling_after_break() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n").unwrap();

        let mut seen = Vec::new();
        for_each_value_until(&path, |v| {
            seen.push(v["a"].as_i64().unwrap());
            if seen.len() == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
        assert_eq!(seen, [1, 2]);
    }

    #[test]
    fn jsonl_missing_file_is_an_error() {
        assert!(for_each_value(Path::new("/nonexistent/x.jsonl"), |_| {}).is_err());
    }
}
