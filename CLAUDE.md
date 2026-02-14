# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## General Behaviour

* Ask clarifying questions if something is not well defined or understood
* Feel free to push back if you think an instruction is missing something or didn't consider all cases
* When adding new features or new modules, or when you think a change is high level enough, update this file with the new documentation.

## Project Overview

`repotool` is a Rust CLI tool for managing and analyzing Git repositories. It provides functionality for scanning local filesystems for repositories, cloning repositories from platforms like GitHub and GitLab, computing file hashes, grabbing repositories into archives, and serving repository metadata via HTTP.

## Build and Test Commands

```bash
# to add a dependency use: (instead of editing Cargo.toml manually)
cargo add <dependency>

# Make sure there are no clippy warnings with
cargo clippy

# Build the project
cargo build

# Build with optimizations
cargo build --release

# Run tests
cargo test

# Run a specific test by name pattern
cargo test <test_name_pattern>

# Run tests with output
cargo test -- --nocapture

# Run with logging (default level is info)
cargo run -- <subcommand>

# Run with verbose/debug logging
RUST_LOG=debug cargo run -- <subcommand>
```

## Architecture

The project is organized into six main command modules, each implementing a distinct subcommand:

### 1. Scan Module (`src/scan.rs`)
Recursively scans local directories to find Git repositories and extract their remote URLs.
- Uses `git2` library to inspect repositories
- Supports configurable depth traversal
- Detects duplicates (same remote URL in multiple locations)
- Can output results to a file or stdout

### 2. Clone Module (`src/clone.rs`)
Generates lists of repositories to clone from platforms like GitHub and GitLab.
- Supports GitHub organizations via `octocrab` and GitLab groups via the `gitlab` crate
- GitLab supports custom instances via `--instance` flag
- Uses async/await with Tokio runtime for API interaction
- Can filter by fork status (exclude forks, only forks, or include all)
- Supports comparison with existing repository lists to avoid duplicates
- Outputs shell commands (e.g., `git clone --mirror <url>`) to a file

### 3. Hash Module (`src/hash.rs`)
Computes SHA256 hashes of files in a directory tree, particularly for machine learning model files and archives.
- Uses parallel processing via `rayon` for performance
- Targets specific file extensions (safetensors, GGUF, ONNX, archives, etc.)
- Uses `infer` crate to detect file types and identify potentially relevant files
- Provides progress bars via `indicatif`
- Outputs structured JSON with file metadata (path, filename, hash, size)
- Supports incremental operation via compare files to skip already-hashed files

### 4. Serve Module (`src/serve.rs`)
HTTP server that checks if repositories exist in archive files.
- Serves two endpoints: `/has_git_repo` and `/has_huggingface_repo`
- Loads newline-separated archive files into in-memory HashSets
- Optional file watching with `notify` to reload archives on changes
- `/has_git_repo` handles URL normalization (different schemes/suffixes) to match repositories regardless of URL format
- Uses `tiny_http` for the HTTP server

### 5. Fsck Module (`src/fsck.rs`)
Runs `git fsck --full --no-dangling` on bare repositories discovered via the scan module.
- Reuses `scan::scan_directory()` to discover repositories with configurable depth
- Verifies each repository is bare before running fsck (aborts if a non-bare repo is found)
- Prints fsck output for every repository to stdout
- Collects failures (non-zero exit code) and optionally writes them to a plain-text output file
- Prints a summary of total repos checked and number of failures

### 6. Grab Module (`src/grab.rs`)
Higher-level command that combines cloning a repository and adding it to an archive atomically.
- Clones a repository (mirror) and registers it in the archive file in one operation
- Parses and normalizes repository URLs from various formats
- Includes unit tests for URL parsing logic

### Main Entry Point (`src/main.rs`)
Uses `clap` with derive macros for CLI argument parsing. Sets up logging with `pretty_env_logger` (defaults to info level).

## Key Dependencies

- **git2**: Git repository inspection
- **octocrab**: GitHub API client (async, requires Tokio runtime)
- **gitlab**: GitLab API client (supports custom instances)
- **rayon**: Parallel iteration for file hashing
- **notify**: File system watching
- **tiny_http**: Lightweight HTTP server
- **clap**: CLI argument parsing with derive API
- **log/pretty_env_logger**: Structured logging
- **walkdir**: Recursive directory traversal
- **sha2**: SHA256 hashing
- **chrono**: Date/time handling
- **serde/serde_json**: Serialization and JSON output

## Development Notes

- The clone module uses async/await with Tokio runtime for GitHub and GitLab API calls, while the rest of the codebase is synchronous
- Logging is pervasive - use `log::info!`, `log::warn!`, `log::error!`, `log::trace!` for output
- The serve module includes unit tests using `rstest` - tests verify URL normalization logic for git repository matching
- The grab module includes unit tests for URL parsing
- The hash module writes incrementally to disk (configurable sync interval) to avoid losing progress on crashes

## Archive Data Format

The scan output / archive format is CSV:
```
remote_url,relative_path,commit_hash,commit_date,last_fetch
```
