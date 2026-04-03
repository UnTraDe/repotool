use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;

use crate::{archive, scan};

#[derive(Args, Debug)]
pub struct FetchParams {
    /// One or more parent directories to scan and fetch
    pub dirs: Vec<PathBuf>,

    /// Path to archive file to update after fetching
    #[arg(long, env = "REPOTOOL_ARCHIVE")]
    archive: Option<PathBuf>,

    /// Base directory for computing relative paths in the archive
    #[arg(long, env = "REPOTOOL_BASE_DIR")]
    base_dir: Option<PathBuf>,

    /// Print git fetch output to stdout
    #[arg(short, long)]
    verbose: bool,
}

pub fn run(params: FetchParams) -> anyhow::Result<()> {
    let dirs: Vec<PathBuf> = if let Some(base) = &params.base_dir {
        params.dirs.iter().map(|d| base.join(d)).collect()
    } else {
        params.dirs.clone()
    };

    for parent in &dirs {
        println!("=== Fetching in {} ===", parent.display());

        if parent.join("HEAD").exists() {
            fetch_repo(parent, None, params.verbose);
        } else {
            let mut entries: Vec<_> = std::fs::read_dir(parent)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .collect();
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let subdir = entry.path();
                println!("{}", subdir.display());
                fetch_repo(parent, Some(&subdir), params.verbose);
            }
        }
    }

    if let Some(archive_path) = &params.archive {
        let base_dir = params.base_dir.as_ref().ok_or_else(|| {
            anyhow::anyhow!("--base-dir is required when --archive is specified")
        })?;
        update_archive(archive_path, base_dir, &dirs)?;
    }

    Ok(())
}

fn update_archive(archive_path: &Path, base_dir: &Path, dirs: &[PathBuf]) -> anyhow::Result<()> {
    // Rescan all fetched dirs to get fresh entries (updated last_repo_fetch, etc.)
    let mut fresh: Vec<archive::Entry> = Vec::new();
    for dir in dirs {
        let mut entries = scan::scan_directory(dir, 2)?;
        fresh.append(&mut entries);
    }

    // Load existing archive (or start empty)
    let mut existing: Vec<archive::Entry> = if archive_path.exists() {
        archive::load_entries(archive_path, base_dir)?
    } else {
        Vec::new()
    };

    // Build index: remote_url → position in existing vec
    let mut index: HashMap<String, usize> = existing
        .iter()
        .enumerate()
        .map(|(i, e)| (e.remote_url.clone(), i))
        .collect();

    let (mut updated, mut added) = (0usize, 0usize);
    for entry in fresh {
        match index.get(&entry.remote_url) {
            Some(&pos) => { existing[pos] = entry; updated += 1; }
            None => {
                index.insert(entry.remote_url.clone(), existing.len());
                existing.push(entry);
                added += 1;
            }
        }
    }

    // Rewrite archive
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(archive_path)?;

    for entry in &existing {
        writeln!(file, "{}", entry.to_csv_line(base_dir))?;
    }

    log::info!("updated archive: {} updated, {} added", updated, added);
    Ok(())
}

/// Run `git fetch --all -p` for a repository.
/// If `git_dir` is `Some`, uses `--git-dir` (for bare repos referenced from a parent dir).
/// If `git_dir` is `None`, uses `-C parent` to run inside the repo itself.
fn fetch_repo(work_dir: &PathBuf, git_dir: Option<&PathBuf>, verbose: bool) {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    if let Some(dir) = git_dir {
        cmd.arg("--git-dir").arg(dir);
    } else {
        cmd.arg("-C").arg(work_dir);
    }

    cmd.args(["fetch", "--all", "-p"]);

    match cmd.output() {
        Ok(output) => {
            if !output.stdout.is_empty() {
                let text = String::from_utf8_lossy(&output.stdout);
                if verbose { print!("{}", text); } else { log::info!("{}", text.trim_end()); }
            }
            if !output.stderr.is_empty() {
                let text = String::from_utf8_lossy(&output.stderr);
                if verbose { eprint!("{}", text); } else { log::info!("{}", text.trim_end()); }
            }
            if !output.status.success() {
                log::warn!(
                    "git fetch exited with {} for {}",
                    output.status,
                    git_dir.unwrap_or(work_dir).display()
                );
            }
        }
        Err(e) => {
            log::warn!(
                "failed to run git fetch for {}: {e}",
                git_dir.unwrap_or(work_dir).display()
            );
        }
    }
}
