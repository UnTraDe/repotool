use clap::Args;
use std::io::Write;
use std::path::{Path, PathBuf};
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

    let mut failure_file = params
        .output_file
        .as_ref()
        .map(|output_path| {
            fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(output_path)
                .map(io::BufWriter::new)
        })
        .transpose()?;

    let mut failure_count = 0usize;

    for entry in &entries {
        let repo = git2::Repository::open(&entry.path)?;
        if !repo.is_bare() {
            log::warn!(
                "repository at '{}' is not a bare repository, skipping",
                entry.path.display()
            );
            let message = "not a bare repository".to_string();
            write_failure(&mut failure_file, &entry.path, &message)?;
            failure_count += 1;
            continue;
        }

        let output = Command::new("git")
            .args(["-C", &entry.path.to_string_lossy(), "fsck", "--full", "--no-dangling", "--progress"])
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
            write_failure(&mut failure_file, &entry.path, &combined)?;
            failure_count += 1;
        }
    }

    log::info!(
        "checked {} repositories, {} failures",
        entries.len(),
        failure_count
    );

    if failure_count > 0 {
        if let Some(output_path) = &params.output_file {
            log::info!("wrote failure details to {}", output_path.display());
        }
    }

    Ok(())
}

fn write_failure(
    file: &mut Option<io::BufWriter<fs::File>>,
    path: &Path,
    output: &str,
) -> anyhow::Result<()> {
    if let Some(file) = file.as_mut() {
        writeln!(file, "=== {} ===", path.display())?;
        write!(file, "{output}")?;
        writeln!(file)?;
        file.flush()?;
    }
    Ok(())
}
