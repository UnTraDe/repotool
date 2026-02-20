use std::collections::HashSet;
use std::io::{self, BufRead};
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
