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

    /// The same rules as rclone `--exclude` patterns, so rclone skips what the planner skips.
    pub fn rclone_excludes(&self) -> Vec<String> {
        let user = self.names.iter().map(String::as_str);
        let mut patterns = Vec::new();
        for name in IGNORED_NAMES.iter().copied().chain(user) {
            patterns.push(format!("{name}/**"));
            patterns.push(name.to_string());
        }
        patterns.extend(TEMP_SUFFIXES.iter().map(|s| format!("*{s}")));
        patterns.extend(TEMP_PREFIXES.iter().map(|p| format!("{p}*")));
        patterns
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
    fn rclone_patterns_cover_folders_and_temp_files() {
        let patterns = IgnoreRules::new().with_name("build").rclone_excludes();
        for expected in [".git/**", "node_modules/**", "build/**", "*.swp", ".~lock.*"] {
            assert!(patterns.iter().any(|p| p == expected), "{expected}");
        }
    }

    #[test]
    fn user_names_are_matched_exactly() {
        let rules = IgnoreRules::new().with_name("build");
        assert!(rules.is_ignored(Path::new("app/build/out.o")));
        assert!(!rules.is_ignored(Path::new("app/build.rs")));
    }
}
