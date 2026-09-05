use clap::{Args, Subcommand};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::{archive, clone, git_url, scan};

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

    /// Grab repositories from GitLab
    Gitlab {
        /// GitLab instance URL (e.g., https://gitlab.com or https://gitlab.archlinux.org)
        #[arg(short, long, default_value = "https://gitlab.com")]
        instance: String,

        #[command(subcommand)]
        operation: GitlabOperation,
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

    /// Grab all repositories from a GitHub user
    User {
        /// User name
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

#[derive(Subcommand, Debug)]
pub enum GitlabOperation {
    /// Grab all repositories from a GitLab group
    Org {
        /// Group name
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

    /// Grab all repositories from a GitLab user
    User {
        /// User name
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

    /// Grab a single repository from GitLab
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
            } => grab_repos(
                &params.archive,
                &params.base_dir,
                &name,
                compare_file,
                filter_forks,
                only_forks,
                |n| {
                    tokio::runtime::Runtime::new()?
                        .block_on(clone::fetch_github_org_repos(n))
                },
            ),
            GithubOperation::User {
                name,
                compare_file,
                filter_forks,
                only_forks,
            } => grab_repos(
                &params.archive,
                &params.base_dir,
                &name,
                compare_file,
                filter_forks,
                only_forks,
                |n| {
                    tokio::runtime::Runtime::new()?
                        .block_on(clone::fetch_github_user_repos(n))
                },
            ),
            GithubOperation::Single { url, output_dir } => {
                grab_single(&params.archive, &params.base_dir, &url, output_dir)
            }
        },
        GrabPlatform::Gitlab { instance, operation } => match operation {
            GitlabOperation::Org {
                name,
                compare_file,
                filter_forks,
                only_forks,
            } => grab_repos(
                &params.archive,
                &params.base_dir,
                &name,
                compare_file,
                filter_forks,
                only_forks,
                |n| {
                    tokio::runtime::Runtime::new()?
                        .block_on(clone::fetch_gitlab_org_repos(&instance, n))
                },
            ),
            GitlabOperation::User {
                name,
                compare_file,
                filter_forks,
                only_forks,
            } => grab_repos(
                &params.archive,
                &params.base_dir,
                &name,
                compare_file,
                filter_forks,
                only_forks,
                |n| {
                    tokio::runtime::Runtime::new()?
                        .block_on(clone::fetch_gitlab_user_repos(&instance, n))
                },
            ),
            GitlabOperation::Single { url, output_dir } => {
                grab_single(&params.archive, &params.base_dir, &url, output_dir)
            }
        },
    }
}

fn grab_repos(
    archive: &std::path::Path,
    base_dir: &std::path::Path,
    name: &str,
    compare_file: Option<PathBuf>,
    filter_forks: bool,
    only_forks: bool,
    fetch_fn: impl FnOnce(&str) -> anyhow::Result<Vec<clone::Entry>>,
) -> anyhow::Result<()> {
    // Create target directory
    let target_dir = base_dir.join(name);
    fs::create_dir_all(&target_dir)?;
    log::info!("created directory: {}", target_dir.display());

    // Load compare file into HashSet (parse as CSV archive format)
    let compare = if let Some(compare_path) = compare_file {
        archive::load_urls(&compare_path)?
    } else {
        HashSet::new()
    };

    // Fetch repos from GitHub API
    let repos = fetch_fn(name)?;
    let total_count = repos.len();

    // Filter by forks
    let repos = clone::filter_by_forks(repos, filter_forks, only_forks);
    let after_fork_filter = repos.len();

    // Filter by compare list
    let repos = git_url::filter_by_compare_list(repos, &compare);

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
        let repo_name = git_url::extract_repo_name(&entry.clone_url, true)
            .unwrap_or_else(|| entry.clone_url.clone());
        let repo_dir = target_dir.join(&repo_name);

        log::info!("cloning {} to {}", entry.clone_url, repo_dir.display());

        match clone_mirror(&entry.clone_url, &repo_dir) {
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

    // Scan directory
    let scanned = scan::scan_directory(&target_dir, 1)?;
    log::info!("scanned {} repositories in directory", scanned.len());

    // Append to archive
    append_to_archive(archive, &scanned, base_dir)?;

    if failure_count > 0 && success_count == 0 {
        anyhow::bail!("all {} clone operations failed", failure_count);
    }

    Ok(())
}

fn grab_single(
    archive_path: &std::path::Path,
    base_dir: &std::path::Path,
    url: &str,
    output_dir: Option<String>,
) -> anyhow::Result<()> {
    // Determine output directory
    let dir_name = output_dir
        .or_else(|| git_url::extract_repo_name(url, true))
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
    append_to_archive(archive_path, &scanned, base_dir)?;

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
    archive_path: &std::path::Path,
    entries: &[archive::Entry],
    base_dir: &std::path::Path,
) -> anyhow::Result<()> {
    // Load existing archive entries for deduplication
    let existing: HashSet<String> = if archive_path.exists() {
        archive::load_urls(archive_path)?
    } else {
        HashSet::new()
    };

    // Build the whole appendix up front and write it in one call. Writing line by line leaves the
    // archive half-updated between syscalls, which a reader — or a file-syncing tool watching the
    // directory — can pick up mid-way.
    let mut appendix = String::new();
    let mut added_count = 0;
    for entry in entries {
        if !git_url::is_in_compare_list(&entry.remote_url, &existing) {
            appendix.push_str(&entry.to_csv_line(base_dir));
            appendix.push('\n');
            added_count += 1;
        } else {
            log::trace!("skipping duplicate: {}", entry.remote_url);
        }
    }

    // Open archive for appending (create if doesn't exist)
    let mut file = OpenOptions::new().create(true).append(true).open(archive_path)?;
    file.write_all(appendix.as_bytes())?;
    file.sync_all()?;

    log::info!("added {} entries to archive", added_count);

    Ok(())
}
