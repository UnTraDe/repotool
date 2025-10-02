# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`repotool` is a Rust CLI tool for managing and analyzing Git repositories. It provides functionality for scanning local filesystems for repositories, cloning repositories from platforms like GitHub, computing file hashes, and serving repository metadata via HTTP.

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

# Run with logging (default level is info)
cargo run -- <subcommand>

# Run with verbose/debug logging
RUST_LOG=debug cargo run -- <subcommand>
```

## Architecture

The project is organized into four main command modules, each implementing a distinct subcommand:

### 1. Scan Module (`src/scan.rs`)
Recursively scans local directories to find Git repositories and extract their remote URLs.
- Uses `git2` library to inspect repositories
- Supports configurable depth traversal
- Detects duplicates (same remote URL in multiple locations)
- Can output results to a file or stdout

### 2. Clone Module (`src/clone.rs`)
Generates lists of repositories to clone from platforms like GitHub.
- Currently supports GitHub organizations (user support is TODO)
- Uses `octocrab` for GitHub API interaction wrapped in a Tokio runtime
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

### Main Entry Point (`src/main.rs`)
Uses `clap` with derive macros for CLI argument parsing. Sets up logging with `pretty_env_logger` (defaults to info level).

## Key Dependencies

- **git2**: Git repository inspection
- **octocrab**: GitHub API client (async, requires Tokio runtime)
- **rayon**: Parallel iteration for file hashing
- **notify**: File system watching
- **tiny_http**: Lightweight HTTP server
- **clap**: CLI argument parsing with derive API
- **log/pretty_env_logger**: Structured logging

## Development Notes

- The clone module's GitHub implementation uses async/await with Tokio runtime, while the rest of the codebase is synchronous
- Logging is pervasive - use `log::info!`, `log::warn!`, `log::error!`, `log::trace!` for output
- The serve module includes unit tests using `rstest` - tests verify URL normalization logic for git repository matching
- The hash module writes incrementally to disk (configurable sync interval) to avoid losing progress on crashes
