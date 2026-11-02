//! Finds the `.code-workspace` file (VS Code family) that contains a project,
//! looking only at the top level of a configurable directory.

use std::fs;
use std::path::{Path, PathBuf};

struct Candidate {
    file: PathBuf,
    name: String,
    folders: Vec<PathBuf>,
}

/// Returns the workspace file in `dir` that lists `project` among its folders.
///
/// Ranking: single-folder workspaces first, then a file name equal to the
/// project's basename, then alphabetical order. Files whose real path escapes
/// `dir` (symlinks) are ignored, as are unreadable or invalid files.
pub fn workspace_for(dir: &Path, project: &Path) -> Option<PathBuf> {
    let dir = fs::canonicalize(dir).ok().filter(|d| d.is_dir())?;
    let real = fs::canonicalize(project).ok()?;
    let base = real.file_name().map(|n| n.to_string_lossy().into_owned());

    let mut matches: Vec<Candidate> = index(&dir)
        .into_iter()
        .filter(|c| c.folders.contains(&real))
        .collect();
    // Stable sort over the alphabetical index keeps the alphabetical tie-break.
    matches.sort_by_key(|c| (c.folders.len() > 1, Some(&c.name) != base.as_ref()));
    matches.into_iter().next().map(|c| c.file)
}

fn index(dir: &Path) -> Vec<Candidate> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".code-workspace"))
        })
        .collect();
    files.sort();

    let mut out = Vec::new();
    for file in files {
        let Ok(file) = fs::canonicalize(&file) else {
            continue;
        };
        if !file.starts_with(dir) || !file.is_file() {
            continue;
        }
        let Some(data) = fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&strip_jsonc(&t)).ok())
        else {
            continue;
        };
        let Some(list) = data.get("folders").and_then(|f| f.as_array()) else {
            continue;
        };
        let parent = file.parent().unwrap_or(dir);
        let folders: Vec<PathBuf> = list
            .iter()
            .filter_map(|f| f.get("path").and_then(|p| p.as_str()))
            .filter_map(|p| {
                let p = Path::new(p);
                let abs = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    parent.join(p)
                };
                fs::canonicalize(abs).ok()
            })
            .collect();
        if folders.is_empty() {
            continue;
        }
        let name = file
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .trim_end_matches(".code-workspace")
                    .to_string()
            })
            .unwrap_or_default();
        out.push(Candidate {
            file,
            name,
            folders,
        });
    }
    out
}

/// Removes `//` and `/* */` comments and trailing commas before `]`/`}`,
/// leaving string contents untouched (the JSONC dialect of VS Code).
pub fn strip_jsonc(text: &str) -> String {
    strip_trailing_commas(&strip_comments(text))
}

/// Copies a string literal starting at `chars[i]` (a quote); returns the index after it.
fn copy_string(chars: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i + 1;
    while j < chars.len() && chars[j] != '"' {
        j += if chars[j] == '\\' { 2 } else { 1 };
    }
    let end = (j + 1).min(chars.len());
    out.extend(&chars[i..end]);
    end
}

fn strip_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '"' {
            i = copy_string(&chars, i, &mut out);
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            i = copy_string(&chars, i, &mut out);
        } else if chars[i] == ',' {
            let mut k = i + 1;
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            if !matches!(chars.get(k), Some(']') | Some('}')) {
                out.push(',');
            }
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}
