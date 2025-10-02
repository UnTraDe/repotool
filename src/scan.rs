use clap::Args;
use git2::Repository;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{collections::HashMap, fs};

#[derive(Args, Debug)]
pub struct ScanParams {
    /// Directory to scan
    #[arg(short, long)]
    directory: PathBuf,

    /// Output file
    #[arg(short, long)]
    output_file: Option<PathBuf>,

    /// Print output
    #[arg(long)]
    print_output: bool,

    /// Print duplicates
    #[arg(long)]
    print_duplicates: bool,

    /// Print irrelevant
    #[arg(long)]
    print_irrelevant: bool,

    /// How deep subdirectories to scan
    #[arg(long, default_value = "3")]
    depth: usize,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    remote_url: String,
    last_commit_hash: String,
    last_commit_date: String,
    last_repo_fetch: String,
}

pub fn scan(params: ScanParams) -> anyhow::Result<()> {
    let (repositories, irrelevant) = local(&params.directory, 0, params.depth - 1)?;
    let duplicates = find_duplicates(&repositories);

    if params.print_output {
        println!("repositories:");
        for e in &repositories {
            println!("{}", e.remote_url);
        }
    }

    if params.print_duplicates {
        println!("duplicates:");
        for e in &duplicates {
            println!("{} ({})", e.remote_url, e.path.display());
        }
    }

    if params.print_irrelevant {
        println!("irrelevant:");
        for i in &irrelevant {
            println!("{}", i.display());
        }
    }

    if let Some(output) = &params.output_file {
        let mut output = io::BufWriter::new(
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output)?,
        );

        for e in &repositories {
            let relative_path = e
                .path
                .strip_prefix(&params.directory)
                .unwrap_or(&e.path)
                .display();
            writeln!(
                output,
                "{},{},{},{},{}",
                e.remote_url,
                relative_path,
                e.last_commit_hash,
                e.last_commit_date,
                e.last_repo_fetch
            )?;
        }
    }

    log::info!(
        "found {} repositories with {} duplicates",
        repositories.len(),
        duplicates.len()
    );

    Ok(())
}

fn local(
    path: &Path,
    current_depth: usize,
    max_depth: usize,
) -> anyhow::Result<(Vec<Entry>, Vec<PathBuf>)> {
    log::trace!(
        "scanning {}... (depth: {current_depth})",
        path.as_os_str().to_string_lossy()
    );

    let mut urls = vec![];
    let mut irrelevant = vec![];

    match fs::read_dir(path) {
        Ok(entries) => {
            for d in entries.filter_map(|d| d.ok()) {
                let path = d.path();
                let path_string = path.as_os_str().to_string_lossy();

                if !d.file_type()?.is_dir() {
                    log::warn!("'{path_string}' is not a directory, skipping...");
                    irrelevant.push(d.path());
                    continue;
                }

                let entry = match Repository::open(d.path()) {
                    Ok(repo) => {
                        log::trace!("found repository: {path_string}");
                        let remotes = repo
                            .remotes()?
                            .iter()
                            .flatten()
                            .map(|r| r.to_owned())
                            .collect::<Vec<String>>();

                        let remote_name = if remotes.iter().any(|r| r == "origin") {
                            "origin".to_owned()
                        } else if let Some(r) = remotes.first() {
                            r.clone()
                        } else {
                            log::error!("no remotes found for '{path_string}', skipping...");
                            continue;
                        };

                        let url = if let Some(url) = repo.find_remote(&remote_name)?.url() {
                            url.to_owned()
                        } else {
                            log::error!(
                                "no url found for remote '{remote_name}' at '{path_string}', skipping..."
                            );
                            continue;
                        };

                        // Get HEAD commit info (for bare repos, resolve HEAD reference)
                        let (commit_hash, commit_date) = match repo.revparse_single("HEAD") {
                            Ok(obj) => {
                                if let Ok(commit) = obj.peel_to_commit() {
                                    let hash = commit.id().to_string();
                                    let commit_time = commit.time();
                                    let date =
                                        chrono::DateTime::from_timestamp(commit_time.seconds(), 0)
                                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                            .unwrap_or_else(|| "unknown".to_string());
                                    (hash, date)
                                } else {
                                    ("unknown".to_string(), "unknown".to_string())
                                }
                            }
                            Err(_) => ("unknown".to_string(), "unknown".to_string()),
                        };

                        // Get last fetch time from FETCH_HEAD
                        let fetch_head_path = d.path().join("FETCH_HEAD");
                        let last_fetch = if fetch_head_path.exists() {
                            fs::metadata(&fetch_head_path)
                                .ok()
                                .and_then(|metadata| metadata.modified().ok())
                                .and_then(|modified| {
                                    modified.duration_since(std::time::UNIX_EPOCH).ok()
                                })
                                .and_then(|duration| {
                                    chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0)
                                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                })
                                .unwrap_or_else(|| "unknown".to_string())
                        } else {
                            "never".to_string()
                        };

                        Entry {
                            path: d.path(),
                            remote_url: url,
                            last_commit_hash: commit_hash,
                            last_commit_date: commit_date,
                            last_repo_fetch: last_fetch,
                        }
                    }
                    Err(e) => {
                        if e.code() == git2::ErrorCode::NotFound {
                            if current_depth < max_depth {
                                log::trace!(
                                    "'{path_string}' is not a git repository, recursing into it..."
                                );

                                let (mut u, mut i) = local(&path, current_depth + 1, max_depth)?;

                                urls.append(&mut u);
                                irrelevant.append(&mut i);
                            } else {
                                log::warn!("'{path_string}' is not a git repository");
                            }
                        } else {
                            anyhow::bail!("failed to open repository: {path_string}: {e}");
                        }

                        continue;
                    }
                };

                log::trace!(
                    "found repository remote: {path_string} ({})",
                    entry.remote_url
                );

                urls.push(entry);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            log::error!(
                "access denied to directory '{}', skipping...",
                path.as_os_str().to_string_lossy()
            );
            return Ok((vec![], vec![]));
        }
        Err(e) => {
            anyhow::bail!(
                "failed to read directory: '{}': {e}",
                path.as_os_str().to_string_lossy()
            );
        }
    }

    Ok((urls, irrelevant))
}

fn find_duplicates(entries: &[Entry]) -> Vec<Entry> {
    let mut occurrences = HashMap::new();

    for e in entries {
        occurrences
            .entry(e.remote_url.clone())
            .and_modify(|o: &mut Vec<Entry>| o.push(e.clone()))
            .or_insert(vec![e.clone()]);
    }

    occurrences
        .into_iter()
        .filter_map(|(_, v)| if v.len() > 1 { Some(v) } else { None })
        .flatten()
        .collect()
}
