//! Paths that are never synced.

use std::path::Path;

const IGNORED_NAMES: &[&str] = &[".git", "node_modules", "__pycache__", ".DS_Store"];
const TEMP_SUFFIXES: &[&str] = &["~", ".swp", ".swx", ".tmp", ".part", ".crdownload"];
const TEMP_PREFIXES: &[&str] = &[".~lock.", ".goutputstream-", "~$"];

/// Built-in rules for VCS folders, dependency folders and editor temp files, plus user-added names.
#[derive(Debug, Clone, Default)]
pub struct IgnoreRules {
    names: Vec<String>,
}

impl IgnoreRules {
    pub fn new() -> Self {
        Self::default()
    }

    /// Also ignores any file or folder with exactly this name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.names.push(name.into());
        self
    }

    /// True if any component of `path` matches a rule, so ignoring a folder ignores its contents.
    pub fn is_ignored(&self, path: &Path) -> bool {
        path.components().any(|c| {
            let name: &str = &c.as_os_str().to_string_lossy();
            IGNORED_NAMES.contains(&name)
                || TEMP_SUFFIXES.iter().any(|s| name.ends_with(s))
                || TEMP_PREFIXES.iter().any(|s| name.starts_with(s))
                || self.names.iter().any(|n| n == name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored(path: &str) -> bool {
        IgnoreRules::new().is_ignored(Path::new(path))
    }

    #[test]
    fn ignores_vcs_and_dependency_folders() {
        assert!(ignored(".git/HEAD"));
        assert!(ignored("project/node_modules/left-pad/index.js"));
    }

    #[test]
    fn ignores_temp_files() {
        assert!(ignored("notes.txt~"));
        assert!(ignored("src/.main.rs.swp"));
        assert!(ignored("docs/.~lock.budget.ods#"));
        assert!(ignored("Downloads/video.mp4.part"));
    }

    #[test]
    fn keeps_normal_files() {
        assert!(!ignored("docs/report.pdf"));
        assert!(!ignored("git-notes.md"));
        assert!(!ignored(".bashrc"));
    }

    #[test]
    fn user_names_are_matched_exactly() {
        let rules = IgnoreRules::new().with_name("build");
        assert!(rules.is_ignored(Path::new("app/build/out.o")));
        assert!(!rules.is_ignored(Path::new("app/build.rs")));
    }
}
