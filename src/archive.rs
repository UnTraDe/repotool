use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub remote_url: String,
    pub last_commit_hash: String,
    pub last_commit_date: String,
    pub last_repo_fetch: String,
}

impl Entry {
    /// Convert entry to CSV line format, with path relative to base_dir
    pub fn to_csv_line(&self, base_dir: &Path) -> String {
        let relative_path = self.path.strip_prefix(base_dir).unwrap_or(&self.path);
        format!(
            "{},{},{},{},{}",
            self.remote_url,
            relative_path.display(),
            self.last_commit_hash,
            self.last_commit_date,
            self.last_repo_fetch
        )
    }

    /// Parse entry from CSV line format (inverse of to_csv_line)
    /// Returns None for empty or malformed lines
    pub fn from_csv_line(line: &str, base_dir: &Path) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let mut parts = line.splitn(5, ',');
        let remote_url = parts.next()?.to_string();
        let relative_path = parts.next()?;
        let last_commit_hash = parts.next()?.to_string();
        let last_commit_date = parts.next()?.to_string();
        let last_repo_fetch = parts.next()?.to_string();

        Some(Entry {
            path: base_dir.join(relative_path),
            remote_url,
            last_commit_hash,
            last_commit_date,
            last_repo_fetch,
        })
    }
}

/// Load all entries from an archive file
pub fn load_entries(path: &Path, base_dir: &Path) -> io::Result<Vec<Entry>> {
    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    Ok(reader
        .lines()
        .filter_map(|line| {
            line.ok()
                .and_then(|l| Entry::from_csv_line(&l, base_dir))
        })
        .collect())
}

/// Atomically replace `path` with `entries`.
///
/// Writes a temporary file in the same directory and renames it over the target, so a reader —
/// or a file-syncing tool watching the directory — sees either the old archive or the new one,
/// never a partial rewrite. Truncating the archive in place instead leaves it empty or
/// half-populated for the whole duration of the write.
pub fn write_entries_atomic(path: &Path, entries: &[Entry], base_dir: &Path) -> io::Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    // Same directory as the target: rename() cannot cross filesystems, and on the NAS the
    // archive and any temp dir may well be different datasets.
    let mut tmp_name = OsString::from(".");
    tmp_name.push(path.file_name().unwrap_or_else(|| "archive".as_ref()));
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp_path = dir.join(tmp_name);

    let result = write_and_rename(&tmp_path, path, entries, base_dir);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

fn write_and_rename(
    tmp_path: &Path,
    path: &Path,
    entries: &[Entry],
    base_dir: &Path,
) -> io::Result<()> {
    let file = std::fs::File::create(tmp_path)?;

    // The temp file is created under the process umask; carry over the original file's mode so
    // the rename does not silently tighten permissions on the archive.
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(tmp_path, meta.permissions())?;
    }

    let mut writer = io::BufWriter::new(file);
    for entry in entries {
        writeln!(writer, "{}", entry.to_csv_line(base_dir))?;
    }
    writer.flush()?;

    // Flush to disk before the rename: otherwise a crash can leave the archive name pointing at
    // a file whose contents were never written.
    writer.get_ref().sync_all()?;
    drop(writer);

    std::fs::rename(tmp_path, path)
}

/// Load URLs from an archive file (CSV format with remote_url as first field)
pub fn load_urls(path: &Path) -> io::Result<HashSet<String>> {
    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let dummy_base = Path::new("");

    Ok(reader
        .lines()
        .filter_map(|line| {
            line.ok()
                .and_then(|l| Entry::from_csv_line(&l, dummy_base))
                .map(|e| e.remote_url)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, path: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            remote_url: url.to_string(),
            last_commit_hash: "abc123".to_string(),
            last_commit_date: "2026-01-01".to_string(),
            last_repo_fetch: "2026-01-02".to_string(),
        }
    }

    #[test]
    fn write_entries_atomic_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo-archive.txt");
        let base = Path::new("/base");

        let entries = vec![
            entry("https://github.com/a/b.git", "/base/b.git"),
            entry("https://github.com/c/d.git", "/base/d.git"),
        ];
        write_entries_atomic(&path, &entries, base).unwrap();

        let loaded = load_entries(&path, base).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].remote_url, "https://github.com/a/b.git");
        assert_eq!(loaded[1].path, PathBuf::from("/base/d.git"));
    }

    #[test]
    fn write_entries_atomic_replaces_existing_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo-archive.txt");
        let base = Path::new("/base");

        write_entries_atomic(&path, &[entry("https://x/1.git", "/base/1.git")], base).unwrap();
        write_entries_atomic(&path, &[entry("https://x/2.git", "/base/2.git")], base).unwrap();

        let loaded = load_entries(&path, base).unwrap();
        assert_eq!(loaded.len(), 1, "second write should replace, not append");
        assert_eq!(loaded[0].remote_url, "https://x/2.git");

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| n != "repo-archive.txt")
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn write_entries_atomic_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo-archive.txt");
        let base = Path::new("/base");

        write_entries_atomic(&path, &[entry("https://x/1.git", "/base/1.git")], base).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();

        write_entries_atomic(&path, &[entry("https://x/2.git", "/base/2.git")], base).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o664, "rename must not tighten the archive's mode");
    }
}
