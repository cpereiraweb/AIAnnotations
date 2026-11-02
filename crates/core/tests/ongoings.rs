//! Resume documents (`docs/ongoing-sessions/*.md`, Inv §7, port of `Ongoings`).

mod support;

use aiannotations_core::ongoings::{self, ONGOING_DIR};
use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};
use support::*;

fn write_md(dir: &Path, name: &str, body: &str, age_secs: u64) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let t = SystemTime::now() - Duration::from_secs(age_secs);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn ongoing_dir(project: &Path) -> std::path::PathBuf {
    let dir = project.join(ONGOING_DIR);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn clean_title_strips_status_prefixes() {
    let c = |s| ongoings::clean_title(s);
    assert_eq!(
        c("🔄 Em andamento — Login OAuth").as_deref(),
        Some("Login OAuth")
    );
    assert_eq!(c("Retomada: X").as_deref(), Some("X"));
    assert_eq!(c("Handoff --- Y").as_deref(), Some("Y"));
    assert_eq!(c("In progress – Z").as_deref(), Some("Z"));
    assert_eq!(c("next session: W").as_deref(), Some("W"));
    assert_eq!(c("PRÓXIMA SESSÃO - V").as_deref(), Some("V"));
    assert_eq!(c("Handoff -- U").as_deref(), Some("U"));
    assert_eq!(c("Resume — "), None);
    assert_eq!(c("Retomada de dados").as_deref(), Some("Retomada de dados"));
    assert_eq!(c("Plain title").as_deref(), Some("Plain title"));
    assert_eq!(c("   "), None);
}

#[test]
fn lists_only_valid_md_files_newest_first() {
    let env = Env::new();
    let project = env.base.join("proj");
    let dir = ongoing_dir(&project);
    write_md(&dir, "a.md", "# A\n", 100);
    write_md(&dir, "b.md", "# Retomada — B\n", 10);
    write_md(&dir, "c d.md", "# C\n", 5);
    write_md(&dir, "notes.txt", "# N\n", 5);
    fs::create_dir_all(dir.join("x.md")).unwrap();

    let list = ongoings::for_project(7, "proj", project.to_str().unwrap());
    let files: Vec<_> = list.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, ["b.md", "a.md"]);
    assert_eq!(list[0].name, "b");
    assert_eq!(list[0].title, "B");
    assert_eq!(list[0].project_id, 7);
    assert_eq!(list[0].project_name, "proj");
    assert!(list[0].modified_ms > list[1].modified_ms);
}

#[test]
fn title_falls_back_to_file_name() {
    let env = Env::new();
    let project = env.base.join("proj");
    let dir = ongoing_dir(&project);
    let mut body = "text\n".repeat(15);
    body.push_str("# Too late\n");
    write_md(&dir, "fix_login-flow.md", &body, 1);

    let list = ongoings::for_project(1, "proj", project.to_str().unwrap());
    assert_eq!(list[0].title, "fix login flow");
}

#[test]
fn title_is_first_heading_within_fifteen_lines() {
    let env = Env::new();
    let project = env.base.join("proj");
    let dir = ongoing_dir(&project);
    let mut body = "text\n".repeat(14);
    body.push_str("# 🔄 Em andamento — Found\n");
    write_md(&dir, "t.md", &body, 1);

    let list = ongoings::for_project(1, "proj", project.to_str().unwrap());
    assert_eq!(list[0].title, "Found");
}

#[test]
fn relative_or_missing_project_path_yields_nothing() {
    let env = Env::new();
    let project = env.base.join("proj");
    ongoing_dir(&project);
    write_md(&project.join(ONGOING_DIR), "a.md", "# A\n", 1);

    assert!(ongoings::for_project(1, "p", "relative/proj").is_empty());
    assert!(ongoings::for_project(1, "p", "").is_empty());
    let missing = env.base.join("nope");
    assert!(ongoings::for_project(1, "p", missing.to_str().unwrap()).is_empty());
    assert!(ongoings::find(1, "p", "relative/proj", "a.md").is_none());
}

#[test]
fn find_rejects_invalid_names() {
    let env = Env::new();
    let project = env.base.join("proj");
    let dir = ongoing_dir(&project);
    write_md(&dir, "a.md", "# A\n", 1);
    write_md(&env.base, "x.md", "# X\n", 1);
    write_md(&dir, "a b.md", "# AB\n", 1);
    let p = project.to_str().unwrap();

    assert!(ongoings::find(1, "proj", p, "../x.md").is_none());
    assert!(ongoings::find(1, "proj", p, "a b.md").is_none());
    assert!(ongoings::find(1, "proj", p, "missing.md").is_none());
    assert_eq!(ongoings::find(1, "proj", p, "a.md").unwrap().title, "A");
}

#[test]
fn for_projects_merges_all_projects_newest_first() {
    let env = Env::new();
    let (p1, p2) = (env.base.join("one"), env.base.join("two"));
    write_md(&ongoing_dir(&p1), "old.md", "# Old\n", 100);
    write_md(&ongoing_dir(&p2), "new.md", "# New\n", 10);
    let id1 = insert_project(&env.conn, p1.to_str().unwrap());
    let id2 = insert_project(&env.conn, p2.to_str().unwrap());
    insert_project(&env.conn, "relative/dir");

    let list = ongoings::for_projects(&env.conn).unwrap();
    let got: Vec<_> = list
        .iter()
        .map(|o| (o.project_id, o.file.as_str()))
        .collect();
    assert_eq!(got, [(id2, "new.md"), (id1, "old.md")]);
}
