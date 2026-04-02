use std::path::PathBuf;
use std::process::Command;

use clap::Args;

#[derive(Args, Debug)]
pub struct FetchParams {
    /// One or more parent directories to scan and fetch
    pub dirs: Vec<PathBuf>,
}

pub fn run(params: FetchParams) -> anyhow::Result<()> {
    for parent in &params.dirs {
        println!("=== Fetching in {} ===", parent.display());

        if parent.join("HEAD").exists() {
            fetch_repo(parent, None);
        } else {
            let mut entries: Vec<_> = std::fs::read_dir(parent)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .collect();
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let subdir = entry.path();
                println!("{}", subdir.display());
                fetch_repo(parent, Some(&subdir));
            }
        }
    }

    Ok(())
}

/// Run `git fetch --all -p` for a repository.
/// If `git_dir` is `Some`, uses `--git-dir` (for bare repos referenced from a parent dir).
/// If `git_dir` is `None`, uses `-C parent` to run inside the repo itself.
fn fetch_repo(work_dir: &PathBuf, git_dir: Option<&PathBuf>) {
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
                log::info!("{}", String::from_utf8_lossy(&output.stdout).trim_end());
            }
            if !output.stderr.is_empty() {
                log::info!("{}", String::from_utf8_lossy(&output.stderr).trim_end());
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
