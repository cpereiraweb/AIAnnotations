//! Active resume documents: the `.md` files in `<project>/docs/ongoing-sessions/` (Inv §7, port
//! of `Ongoings`). Being in the folder is what makes one active. Read from disk on every call.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::Connection;
use serde::Serialize;

use crate::Result;

pub const ONGOING_DIR: &str = "docs/ongoing-sessions";

/// Lines read from the top of the file looking for the title.
const TITLE_LINES: usize = 15;

/// Status words that may prefix a title (matched without regard to case).
const PREFIX_WORDS: [&str; 7] = [
    "Em andamento",
    "Retomada",
    "Handoff",
    "Próxima sessão",
    "In progress",
    "Resume",
    "Next session",
];

/// Separators after the status word, longest first so `---` wins over `--` over `-`.
const SEPARATORS: [&str; 6] = ["---", "--", "-", "—", "–", ":"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ongoing {
    pub project_id: i64,
    pub project_name: String,
    pub project_path: String,
    pub file: String,
    /// `file` without `.md`.
    pub name: String,
    pub title: String,
    pub modified_ms: i64,
}

/// The project's ongoings, newest first.
pub fn for_project(project_id: i64, project_name: &str, project_path: &str) -> Vec<Ongoing> {
    let Some(dir) = directory(project_path) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut list: Vec<Ongoing> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| valid_file_name(f))
        .filter_map(|f| make(project_id, project_name, project_path, &dir, f))
        .collect();
    sort_newest_first(&mut list);
    list
}

/// The ongoings of every project in the database, newest first.
pub fn for_projects(conn: &Connection) -> Result<Vec<Ongoing>> {
    let mut stmt = conn.prepare("SELECT id, name, path FROM projects")?;
    let projects = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut list: Vec<Ongoing> = projects
        .iter()
        .flat_map(|(id, name, path)| for_project(*id, name, path))
        .collect();
    sort_newest_first(&mut list);
    Ok(list)
}

/// The ongoing named `file` in the project, or `None` (invalid name or missing file).
pub fn find(
    project_id: i64,
    project_name: &str,
    project_path: &str,
    file: &str,
) -> Option<Ongoing> {
    let dir = directory(project_path)?;
    if !valid_file_name(file) {
        return None;
    }
    make(
        project_id,
        project_name,
        project_path,
        &dir,
        file.to_string(),
    )
}

/// Title text without the optional `🔄`, a status word and its separator ("🔄 Em andamento — X"
/// gives "X"). Without a separator nothing is cut. `None` when nothing is left.
pub fn clean_title(heading: &str) -> Option<String> {
    let heading = heading.trim();
    let stripped = strip_status(heading).unwrap_or(heading).trim();
    (!stripped.is_empty()).then(|| stripped.to_string())
}

fn strip_status(heading: &str) -> Option<&str> {
    let rest = heading.strip_prefix('🔄').unwrap_or(heading).trim_start();
    PREFIX_WORDS.iter().find_map(|word| {
        let after = strip_prefix_ignore_case(rest, word)?.trim_start();
        let after = SEPARATORS.iter().find_map(|sep| after.strip_prefix(sep))?;
        Some(after.trim_start())
    })
}

/// `s` without the leading `prefix`, comparing characters by their lowercase form.
fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut chars = s.char_indices();
    for p in prefix.chars() {
        let (_, c) = chars.next()?;
        if !c.to_lowercase().eq(p.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or("", |(i, _)| &s[i..]))
}

/// Same as `^[A-Za-z0-9._-]+\.md\z`: the name goes into handler arguments, so only simple names.
fn valid_file_name(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

fn directory(project_path: &str) -> Option<PathBuf> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return None;
    }
    let dir = path.join(ONGOING_DIR);
    dir.is_dir().then_some(dir)
}

fn make(
    project_id: i64,
    project_name: &str,
    project_path: &str,
    dir: &Path,
    file: String,
) -> Option<Ongoing> {
    let path = dir.join(&file);
    let meta = fs::metadata(&path).ok().filter(|m| m.is_file())?;
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64);
    let name = file.strip_suffix(".md").unwrap_or(&file).to_string();
    let title = read_title(&path).unwrap_or_else(|| name.replace(['-', '_'], " "));
    Some(Ongoing {
        project_id,
        project_name: project_name.to_string(),
        project_path: project_path.to_string(),
        file,
        name,
        title,
        modified_ms,
    })
}

/// First `# …` heading among the first lines of the file, cleaned.
fn read_title(path: &Path) -> Option<String> {
    let reader = BufReader::new(fs::File::open(path).ok()?);
    for line in reader.lines().take(TITLE_LINES) {
        let line = line.ok()?;
        let line = line.trim();
        if let Some(heading) = line.strip_prefix('#')
            && heading.starts_with(char::is_whitespace)
            && !heading.trim().is_empty()
        {
            return clean_title(heading);
        }
    }
    None
}

fn sort_newest_first(list: &mut [Ongoing]) {
    list.sort_by_key(|o| std::cmp::Reverse(o.modified_ms));
}
