//! In-memory probe for tests: lets every OS's detection run on any host.
//! Paths are plain strings and are never joined or split with the host separator.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Os;
use super::detect::Probe;

#[derive(Debug, Clone)]
pub struct FakeProbe {
    os: Os,
    env: HashMap<String, OsString>,
    files: HashMap<PathBuf, String>,
    dirs: HashSet<PathBuf>,
    links: HashMap<PathBuf, PathBuf>,
    app_paths: HashMap<String, PathBuf>,
}

impl FakeProbe {
    pub fn new(os: Os) -> Self {
        FakeProbe {
            os,
            env: HashMap::new(),
            files: HashMap::new(),
            dirs: HashSet::new(),
            links: HashMap::new(),
            app_paths: HashMap::new(),
        }
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), OsString::from(value));
        self
    }

    pub fn file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(PathBuf::from(path), content.to_string());
        self
    }

    pub fn dir(mut self, path: &str) -> Self {
        self.dirs.insert(PathBuf::from(path));
        self
    }

    pub fn link(mut self, path: &str, target: &str) -> Self {
        self.links
            .insert(PathBuf::from(path), PathBuf::from(target));
        self
    }

    pub fn app_path(mut self, exe: &str, path: &str) -> Self {
        self.app_paths.insert(exe.to_string(), PathBuf::from(path));
        self
    }

    /// Follows symlinks (at most 16 hops).
    fn resolve(&self, path: &Path) -> PathBuf {
        let mut cur = path.to_path_buf();
        for _ in 0..16 {
            match self.links.get(&cur) {
                Some(next) => cur = next.clone(),
                None => break,
            }
        }
        cur
    }

    fn parent_of(path: &Path) -> String {
        let s = path.to_string_lossy();
        s.rfind(['/', '\\'])
            .map_or(String::new(), |i| s[..i].to_string())
    }
}

impl Probe for FakeProbe {
    fn os(&self) -> Os {
        self.os
    }

    fn env(&self, key: &str) -> Option<OsString> {
        self.env.get(key).cloned()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&self.resolve(path))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains(&self.resolve(path))
    }

    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        let want = path
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        let mut out: Vec<PathBuf> = self
            .files
            .keys()
            .chain(self.dirs.iter())
            .filter(|p| Self::parent_of(p) == want)
            .cloned()
            .collect();
        out.sort();
        out
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        self.files.get(&self.resolve(path)).cloned()
    }

    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let r = self.resolve(path);
        (self.files.contains_key(&r) || self.dirs.contains(&r)).then_some(r)
    }

    fn app_path(&self, exe: &str) -> Option<PathBuf> {
        self.app_paths.get(exe).cloned()
    }
}
