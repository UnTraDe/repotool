use clap::Args;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::{fs, io};

use crate::scan;

#[derive(Args, Debug)]
pub struct FsckParams {
    /// Directory to scan for bare repositories
    #[arg(short, long)]
    directory: PathBuf,

    /// Output file for failed fsck results
    #[arg(short, long)]
    output_file: Option<PathBuf>,

    /// How deep subdirectories to scan
    #[arg(long, default_value = "3")]
    depth: usize,
}

pub fn run(params: FsckParams) -> anyhow::Result<()> {
    let entries = scan::scan_directory(&params.directory, params.depth)?;

    if entries.is_empty() {
        log::info!("no repositories found in {}", params.directory.display());
        return Ok(());
    }

    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    for entry in &entries {
        let repo = git2::Repository::open(&entry.path)?;
        if !repo.is_bare() {
            anyhow::bail!(
                "repository at '{}' is not a bare repository, aborting",
                entry.path.display()
            );
        }

        let output = Command::new("git")
            .args(["-C", &entry.path.to_string_lossy(), "fsck", "--full", "--no-dangling"])
            .output()?;

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        println!("=== {} ===", entry.path.display());
        print!("{combined}");
        println!();

        if !output.status.success() {
            failures.push((entry.path.clone(), combined));
        }
    }

    log::info!(
        "checked {} repositories, {} failures",
        entries.len(),
        failures.len()
    );

    if let Some(output_path) = &params.output_file {
        if !failures.is_empty() {
            let mut file = io::BufWriter::new(
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(output_path)?,
            );

            for (path, output) in &failures {
                writeln!(file, "=== {} ===", path.display())?;
                write!(file, "{output}")?;
                writeln!(file)?;
            }

            log::info!("wrote failure details to {}", output_path.display());
        }
    }

    Ok(())
}
