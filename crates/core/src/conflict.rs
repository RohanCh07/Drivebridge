//! Names for the second copy when a file changed on both sides.

use std::path::{Path, PathBuf};

/// `docs/report.pdf` becomes `docs/report (conflict 2026-10-29 laptop).pdf`.
pub fn conflict_name(path: &Path, date: &str, host: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let mut name = format!("{stem} (conflict {date} {host})");
    if let Some(ext) = path.extension() {
        name.push('.');
        name.push_str(&ext.to_string_lossy());
    }
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(path: &str) -> PathBuf {
        conflict_name(Path::new(path), "2026-10-29", "laptop")
    }

    #[test]
    fn keeps_folder_and_extension() {
        assert_eq!(
            name("docs/report.pdf"),
            PathBuf::from("docs/report (conflict 2026-10-29 laptop).pdf")
        );
    }

    #[test]
    fn handles_files_without_extension() {
        assert_eq!(
            name("Makefile"),
            PathBuf::from("Makefile (conflict 2026-10-29 laptop)")
        );
    }

    #[test]
    fn handles_dotfiles() {
        assert_eq!(
            name(".bashrc"),
            PathBuf::from(".bashrc (conflict 2026-10-29 laptop)")
        );
    }

    #[test]
    fn only_last_extension_moves() {
        assert_eq!(
            name("archive.tar.gz"),
            PathBuf::from("archive.tar (conflict 2026-10-29 laptop).gz")
        );
    }
}
