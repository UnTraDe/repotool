use clap::{Args, Subcommand};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{clone, scan};

/// Load URLs from an archive file (CSV format with remote_url as first field)
fn load_urls_from_archive(path: &Path) -> io::Result<HashSet<String>> {
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let dummy_base = Path::new("");

    Ok(reader
        .lines()
        .filter_map(|line| {
            line.ok()
                .and_then(|l| scan::Entry::from_csv_line(&l, dummy_base))
                .map(|e| e.remote_url)
        })
        .collect())
}

#[derive(Args, Debug)]
pub struct GrabParams {
    /// Path to archive file to append results
    #[arg(long, env = "REPOTOOL_ARCHIVE")]
    archive: PathBuf,

    /// Base directory for cloning operations
    #[arg(long, env = "REPOTOOL_BASE_DIR")]
    base_dir: PathBuf,

    #[command(subcommand)]
    platform: GrabPlatform,
}

#[derive(Subcommand, Debug)]
pub enum GrabPlatform {
    /// Grab repositories from GitHub
    Github {
        #[command(subcommand)]
        operation: GithubOperation,
    },
}

#[derive(Subcommand, Debug)]
pub enum GithubOperation {
    /// Grab all repositories from a GitHub organization
    Org {
        /// Organization name
        name: String,

        /// Compare repository list with a given file, and skip repos already in it
        #[arg(long)]
        compare_file: Option<PathBuf>,

        /// Filter out forks
        #[arg(long, group = "forks")]
        filter_forks: bool,

        /// Only clone forks
        #[arg(long, group = "forks")]
        only_forks: bool,
    },

    /// Grab a single repository from GitHub
    Single {
        /// Repository URL
        url: String,

        /// Override default directory name (extracted from URL)
        #[arg(long)]
        output_dir: Option<String>,
    },
}

pub fn run(params: GrabParams) -> anyhow::Result<()> {
    match params.platform {
        GrabPlatform::Github { operation } => match operation {
            GithubOperation::Org {
                name,
                compare_file,
                filter_forks,
                only_forks,
            } => grab_github_org(
                &params.archive,
                &params.base_dir,
                &name,
                compare_file,
                filter_forks,
                only_forks,
            ),
            GithubOperation::Single { url, output_dir } => {
                grab_github_single(&params.archive, &params.base_dir, &url, output_dir)
            }
        },
    }
}

fn grab_github_org(
    archive: &std::path::Path,
    base_dir: &std::path::Path,
    org_name: &str,
    compare_file: Option<PathBuf>,
    filter_forks: bool,
    only_forks: bool,
) -> anyhow::Result<()> {
    // Create org directory
    let org_dir = base_dir.join(org_name);
    fs::create_dir_all(&org_dir)?;
    log::info!("created org directory: {}", org_dir.display());

    // Load compare file into HashSet (parse as CSV archive format)
    let compare = if let Some(compare_path) = compare_file {
        load_urls_from_archive(&compare_path)?
    } else {
        HashSet::new()
    };

    // Fetch repos from GitHub API
    let runtime = tokio::runtime::Runtime::new()?;
    let repos = runtime.block_on(clone::fetch_github_org_repos(org_name))?;
    let total_count = repos.len();

    // Filter by forks
    let repos = clone::filter_by_forks(repos, filter_forks, only_forks);
    let after_fork_filter = repos.len();

    // Filter by compare list
    let repos = clone::filter_by_compare_list(repos, &compare);

    log::info!(
        "found {} repositories, {} after fork filter, {} after compare filter",
        total_count,
        after_fork_filter,
        repos.len()
    );

    // Clone each repo
    let mut success_count = 0;
    let mut failure_count = 0;

    for entry in &repos {
        let repo_name = extract_repo_name_from_url(&entry.clone_url, true)
            .unwrap_or_else(|| entry.clone_url.clone());
        let target_dir = org_dir.join(&repo_name);

        log::info!("cloning {} to {}", entry.clone_url, target_dir.display());

        match clone_mirror(&entry.clone_url, &target_dir) {
            Ok(()) => {
                log::info!("successfully cloned {}", repo_name);
                success_count += 1;
            }
            Err(e) => {
                log::error!("failed to clone {}: {}", repo_name, e);
                failure_count += 1;
            }
        }
    }

    log::info!(
        "cloning complete: {} succeeded, {} failed",
        success_count,
        failure_count
    );

    // Scan org directory
    let scanned = scan::scan_directory(&org_dir, 1)?;
    log::info!("scanned {} repositories in org directory", scanned.len());

    // Append to archive
    append_to_archive(archive, &scanned, base_dir)?;

    if failure_count > 0 && success_count == 0 {
        anyhow::bail!("all {} clone operations failed", failure_count);
    }

    Ok(())
}

fn grab_github_single(
    archive: &std::path::Path,
    base_dir: &std::path::Path,
    url: &str,
    output_dir: Option<String>,
) -> anyhow::Result<()> {
    // Determine output directory
    let dir_name = output_dir
        .or_else(|| extract_repo_name_from_url(url, true))
        .ok_or_else(|| anyhow::anyhow!("could not determine output directory from URL: {}", url))?;

    let target_dir = base_dir.join(&dir_name);

    log::info!("cloning {} to {}", url, target_dir.display());

    // Clone the repository
    clone_mirror(url, &target_dir)?;

    log::info!("successfully cloned {}", dir_name);

    // Scan the cloned directory
    let scanned = scan::scan_directory(&target_dir, 1)?;
    log::info!("scanned {} repositories", scanned.len());

    // Append to archive
    append_to_archive(archive, &scanned, base_dir)?;

    Ok(())
}

fn clone_mirror(url: &str, target_dir: &std::path::Path) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(["clone", "--mirror", url])
        .arg(target_dir)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr);
    }

    Ok(())
}

fn append_to_archive(
    archive: &std::path::Path,
    entries: &[scan::Entry],
    base_dir: &std::path::Path,
) -> anyhow::Result<()> {
    // Load existing archive entries for deduplication
    let existing: HashSet<String> = if archive.exists() {
        load_urls_from_archive(archive)?
    } else {
        HashSet::new()
    };

    // Open archive for appending (create if doesn't exist)
    let mut file = OpenOptions::new().create(true).append(true).open(archive)?;

    let mut added_count = 0;
    for entry in entries {
        if !clone::is_in_compare_list(&entry.remote_url, &existing) {
            writeln!(file, "{}", entry.to_csv_line(base_dir))?;
            added_count += 1;
        } else {
            log::trace!("skipping duplicate: {}", entry.remote_url);
        }
    }

    log::info!("added {} entries to archive", added_count);

    Ok(())
}

/// Extract repository name from a URL
/// Handles formats like:
/// - https://github.com/owner/repo.git
/// - https://github.com/owner/repo
/// - git@github.com:owner/repo.git
pub fn extract_repo_name_from_url(url: &str, preserve_dot_git: bool) -> Option<String> {
    // Handle SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some(path) = rest.split(':').nth(1) {
            let mut name = path.rsplit('/').next()?.to_string();

            if !preserve_dot_git {
                name = name.trim_end_matches(".git").to_string()
            }

            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    // Handle HTTPS format: https://github.com/owner/repo.git
    let path = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    let mut name = path.trim_end_matches('/').rsplit('/').next()?.to_string();

    if !preserve_dot_git {
        name = name.trim_end_matches(".git").to_string()
    }

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_name_https_with_git() {
        assert_eq!(
            extract_repo_name_from_url("https://github.com/owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_without_git() {
        assert_eq!(
            extract_repo_name_from_url("https://github.com/owner/repo", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_trailing_slash() {
        assert_eq!(
            extract_repo_name_from_url("https://github.com/owner/repo/", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh_with_git() {
        assert_eq!(
            extract_repo_name_from_url("git@github.com:owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh_without_git() {
        assert_eq!(
            extract_repo_name_from_url("git@github.com:owner/repo", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_http() {
        assert_eq!(
            extract_repo_name_from_url("http://github.com/owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_with_git_preserve() {
        assert_eq!(
            extract_repo_name_from_url("https://github.com/owner/repo.git", true),
            Some("repo.git".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_without_git_preserve() {
        assert_eq!(
            extract_repo_name_from_url("https://github.com/owner/repo", true),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_invalid() {
        assert_eq!(extract_repo_name_from_url("not-a-url", false), None);
    }
}
