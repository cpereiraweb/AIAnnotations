//! File-based event queue written by the hook and drained by ingestion (Inv §3.1).
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes one envelope atomically (`.<name>.tmp` then rename) and returns its final path.
///
/// `stdin` that is empty or not valid JSON is recorded as a `null` payload.
pub fn write_envelope(
    spool_dir: &Path,
    tool: &str,
    event: &str,
    stdin: &[u8],
    now: DateTime<Utc>,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(spool_dir)?;
    let payload: Value = serde_json::from_slice(stdin).unwrap_or(Value::Null);
    let envelope = json!({
        "tool": tool,
        "event": event,
        "received_at": now.to_rfc3339_opts(SecondsFormat::Nanos, true),
        "payload": payload,
    });
    let epoch_ns = now.timestamp_nanos_opt().unwrap_or_default();
    let name = format!("{epoch_ns}-{tool}-{event}-{}", std::process::id());
    let tmp = spool_dir.join(format!(".{name}.tmp"));
    let final_path = spool_dir.join(format!("{name}.json"));

    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec(&envelope).map_err(std::io::Error::other)?)?;
        f.write_all(b"\n")?;
        drop(f);
        fs::rename(&tmp, &final_path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(final_path)
}

/// `*.json` files in the spool root (temp files and subfolders excluded), in name order.
pub fn pending(spool_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(spool_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") && entry.file_type()?.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 29, 18, 14, 50).unwrap()
            + chrono::Duration::nanoseconds(123_456_789)
    }

    #[test]
    fn envelope_has_nanosecond_utc_timestamp_and_name() {
        let tmp = tempfile::tempdir().unwrap();
        let p =
            write_envelope(&tmp.path().join("s"), "claude", "start", b"{\"a\":1}", at()).unwrap();
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("1793297690123456789-claude-start-"),
            "{name}"
        );
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["received_at"], "2026-10-29T18:14:50.123456789Z");
        assert_eq!(v["payload"], json!({"a":1}));
    }

    #[test]
    fn pending_lists_only_json_in_name_order() {
        let tmp = tempfile::tempdir().unwrap();
        for n in ["b.json", "a.json", ".c.tmp", "d.txt"] {
            fs::write(tmp.path().join(n), "x").unwrap();
        }
        fs::create_dir(tmp.path().join("failed.json")).unwrap();
        let names: Vec<_> = pending(tmp.path())
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["a.json", "b.json"]);
    }
}
