use clap::Parser;
use notify::Watcher;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
};
use tiny_http::{Response, Server};

#[derive(Parser, Debug)]
pub struct ServeParams {
    /// Address to bind the server to
    #[arg(long, default_value = "127.0.0.1")]
    address: String,

    /// Port to listen on
    #[arg(long, default_value_t = 8081)]
    port: u16,

    /// Newline-seperated archive file
    #[arg(long)]
    git_repo_archive: PathBuf,

    /// Newline-seperated archive file
    #[arg(long)]
    huggingface_archive: Option<PathBuf>,

    /// Watch the archive file for changes
    #[arg(long)]
    watch: bool,
}

#[derive(serde::Deserialize, Debug)]
struct HasGitRepoRequest {
    url: String,
}

#[derive(serde::Deserialize, Debug)]
struct HasHuggingfaceRequest {
    repo: String,
}

struct ArchiveHandle {
    archive: Arc<Mutex<HashMap<String, RepoMetadata>>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

struct HuggingfaceArchiveHandle {
    archive: Arc<Mutex<HashSet<String>>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

#[derive(Clone, Debug)]
struct RepoMetadata {
    url: String,
    path: String,
    commit_hash: String,
    commit_date: String,
    last_fetch: String,
}

fn read_and_parse_git_archive(path: &Path) -> anyhow::Result<HashMap<String, RepoMetadata>> {
    let map: HashMap<String, RepoMetadata> = std::fs::read_to_string(path)?
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() != 5 {
                panic!("bad line: {line}")
            }

            let metadata = RepoMetadata {
                url: parts[0].to_string(),
                path: parts[1].to_string(),
                commit_hash: parts[2].to_string(),
                commit_date: parts[3].to_string(),
                last_fetch: parts[4].to_string(),
            };

            (metadata.url.to_lowercase(), metadata)
        })
        .collect();

    Ok(map)
}

fn read_and_parse_huggingface_archive(path: &Path) -> anyhow::Result<HashSet<String>> {
    let urls: HashSet<String> = std::fs::read_to_string(path)?
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(urls)
}

fn load_git_archive(path: &Path, watch: bool) -> anyhow::Result<ArchiveHandle> {
    let map = read_and_parse_git_archive(path)?;
    let archive = Arc::new(Mutex::new(map));

    let watcher = if watch {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(path, notify::RecursiveMode::Recursive)?;

        let archive_clone = archive.clone();
        let path = path.to_path_buf();

        std::thread::spawn(move || loop {
            match rx.recv() {
                Ok(event) => match event {
                    Ok(event) => {
                        log::trace!("event: {:?}", event);
                        let map = read_and_parse_git_archive(&path).unwrap();
                        *archive_clone.lock().unwrap() = map;
                    }
                    Err(e) => log::error!("watch error: {:?}", e),
                },
                Err(e) => {
                    log::error!("watch error: {:?}", e);
                    break;
                }
            }
        });

        Some(watcher)
    } else {
        None
    };

    Ok(ArchiveHandle {
        archive,
        _watcher: watcher,
    })
}

fn load_huggingface_archive(path: &Path, watch: bool) -> anyhow::Result<HuggingfaceArchiveHandle> {
    let urls = read_and_parse_huggingface_archive(path)?;
    let archive = Arc::new(Mutex::new(urls));

    let watcher = if watch {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(path, notify::RecursiveMode::Recursive)?;

        let archive_clone = archive.clone();
        let path = path.to_path_buf();

        std::thread::spawn(move || loop {
            match rx.recv() {
                Ok(event) => match event {
                    Ok(event) => {
                        log::trace!("event: {:?}", event);
                        let urls = read_and_parse_huggingface_archive(&path).unwrap();
                        *archive_clone.lock().unwrap() = urls;
                    }
                    Err(e) => log::error!("watch error: {:?}", e),
                },
                Err(e) => {
                    log::error!("watch error: {:?}", e);
                    break;
                }
            }
        });

        Some(watcher)
    } else {
        None
    };

    Ok(HuggingfaceArchiveHandle {
        archive,
        _watcher: watcher,
    })
}

fn handle_has_git_repo_req(
    req: HasGitRepoRequest,
    archive_handle: &ArchiveHandle,
) -> anyhow::Result<Response<std::io::Cursor<Vec<u8>>>> {
    log::info!("handle_has_git_repo_req: {req:?}");

    let variants = {
        let schemas = &["http://", "https://", "git://"];
        let suffixes = &[".git"];

        let original = req.url.to_lowercase();
        let suffix_stripped = suffixes
            .iter()
            .filter_map(|s| original.strip_suffix(s))
            .collect::<Vec<_>>();

        if suffix_stripped.len() > 1 {
            anyhow::bail!("logic error");
        }

        let suffix_stripped = suffix_stripped
            .first()
            .map_or(original.clone(), |v| v.to_string());

        let suffix_stripped_cloned = suffix_stripped.clone();
        let schema_stripped = schemas
            .iter()
            .filter_map(|s| suffix_stripped_cloned.strip_prefix(s))
            .collect::<Vec<_>>();

        if schema_stripped.len() > 1 {
            anyhow::bail!("logic error");
        }

        let stripped = schema_stripped
            .first()
            .map_or(suffix_stripped, |v| v.to_string());

        let mut variants = vec![];

        for schema in schemas {
            let mut with_schema = stripped.clone();
            with_schema.insert_str(0, schema);

            for suffix in suffixes {
                variants.push(with_schema.clone() + suffix);
            }

            variants.push(with_schema);
        }

        variants.push(stripped);

        variants
    };

    let (existing, metadata) = {
        let archive = archive_handle.archive.lock().unwrap();

        let mut existing = Vec::new();
        let mut metadata = None;

        for variant in &variants {
            if let Some(repo_meta) = archive.get(variant) {
                existing.push(repo_meta.url.clone());
                if metadata.is_none() {
                    metadata = Some(repo_meta.clone());
                }
            }
        }

        (existing, metadata)
    };

    let response = if let Some(meta) = metadata {
        json!({
            "exists": !existing.is_empty(),
            "existing": existing,
            "metadata": {
                "url": meta.url,
                "path": meta.path,
                "commit_hash": meta.commit_hash,
                "commit_date": meta.commit_date,
                "last_fetch": meta.last_fetch
            }
        })
    } else {
        json!({
            "exists": !existing.is_empty(),
            "existing": existing
        })
    };

    log::debug!("response: {response}");

    Ok(Response::from_string(response.to_string()))
}

fn handle_has_huggingface_repo_req(
    req: HasHuggingfaceRequest,
    archive_handle: &HuggingfaceArchiveHandle,
) -> anyhow::Result<Response<std::io::Cursor<Vec<u8>>>> {
    log::info!("handle_has_huggingface_repo_req: {req:?}");

    let exists = {
        let archive = archive_handle.archive.lock().unwrap();
        archive
            .iter()
            .any(|repo| repo.to_lowercase() == req.repo.to_lowercase())
    };

    let response = json!({
        "exists": exists
    });

    log::debug!("response: {response}");

    Ok(Response::from_string(response.to_string()))
}

pub fn run(args: ServeParams) -> anyhow::Result<()> {
    let git_repo_archive = load_git_archive(&args.git_repo_archive, args.watch)?;
    let huggingface_archive = if let Some(archive) = args.huggingface_archive {
        Some(load_huggingface_archive(&archive, args.watch)?)
    } else {
        None
    };

    let server = Server::http(format!("{}:{}", args.address, args.port))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    log::info!("Server listening on {}:{}", args.address, args.port);

    for mut request in server.incoming_requests() {
        log::info!("got request: {}", request.url());

        match request.url() {
            "/has_git_repo" => {
                match serde_json::from_reader::<_, HasGitRepoRequest>(request.as_reader()) {
                    Ok(req) => match handle_has_git_repo_req(req, &git_repo_archive) {
                        Ok(r) => request.respond(r)?,
                        Err(e) => request.respond(Response::from_string(
                            json!({
                                "error": "error handling request",
                                "details": e.to_string()
                            })
                            .to_string(),
                        ))?,
                    },
                    Err(e) => {
                        log::warn!("json parse error: {}", e.to_string());
                        request.respond(Response::from_string(
                            json!({
                                "error": "json parse error",
                                "details": e.to_string()
                            })
                            .to_string(),
                        ))?;
                    }
                }
            }
            "/has_huggingface_repo" => {
                match serde_json::from_reader::<_, HasHuggingfaceRequest>(request.as_reader()) {
                    Ok(req) => {
                        if let Some(huggingface_archive) = &huggingface_archive {
                            match handle_has_huggingface_repo_req(req, huggingface_archive) {
                                Ok(r) => request.respond(r)?,
                                Err(e) => request.respond(Response::from_string(
                                    json!({
                                        "error": "error handling request",
                                        "details": e.to_string()
                                    })
                                    .to_string(),
                                ))?,
                            }
                        } else {
                            request.respond(Response::from_string(
                                json!({
                                    "error": "instance started without huggingface archive provided"
                                })
                                .to_string(),
                            ))?
                        }
                    }
                    Err(e) => {
                        log::warn!("json parse error: {}", e.to_string());
                        request.respond(Response::from_string(
                            json!({
                                "error": "json parse error",
                                "details": e.to_string()
                            })
                            .to_string(),
                        ))?;
                    }
                }
            }
            _ => {
                log::warn!("invalid endpoint: {}", request.url());
                request.respond(Response::from_string(
                    json!({
                        "error": "invalid endpoint"
                    })
                    .to_string(),
                ))?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::HasGitRepoRequest;
    use rstest::{fixture, rstest};
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };

    #[fixture]
    pub fn archive() -> super::ArchiveHandle {
        let urls = [
            "https://github.com/rust-lang/rust.git",
            "http://github.com/rust-lang/rust.git",
            "git://github.com/rust-lang/rust",
            "github.com/rust-lang/rust",
            "https://github.com/rust-lang/rust-clippy.git",
            "http://github.com/rust-lang/rust-clippy",
            "git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git",
        ];

        let map: std::collections::HashMap<String, super::RepoMetadata> = urls
            .iter()
            .map(|url| {
                let metadata = super::RepoMetadata {
                    url: url.to_string(),
                    path: format!("{}.git", url.split('/').next_back().unwrap_or("repo")),
                    commit_hash: "abc123".to_string(),
                    commit_date: "2025-01-01 12:00:00".to_string(),
                    last_fetch: "never".to_string(),
                };
                (url.to_string(), metadata)
            })
            .collect();

        super::ArchiveHandle {
            archive: Arc::new(Mutex::new(map)),
            _watcher: None,
        }
    }

    #[rstest]
    #[case("https://github.com/rust-lang/rust.git", &["https://github.com/rust-lang/rust.git", "http://github.com/rust-lang/rust.git", "git://github.com/rust-lang/rust", "github.com/rust-lang/rust"])]
    #[case("github.com/rust-lang/rust-clippy", &[ "https://github.com/rust-lang/rust-clippy.git", "http://github.com/rust-lang/rust-clippy"])]
    #[case("https://github.com/rust-lang/miri.git", &[])]
    #[case("git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git", &["git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git"])]
    #[case("HTTPS://GITHUB.COM/rust-lang/rust.git", &["https://github.com/rust-lang/rust.git", "http://github.com/rust-lang/rust.git", "git://github.com/rust-lang/rust", "github.com/rust-lang/rust"])]
    #[case("GITHUB.COM/RUST-LANG/RUST-CLIPPY", &[ "https://github.com/rust-lang/rust-clippy.git", "http://github.com/rust-lang/rust-clippy"])]
    fn handle_has_git_repo_req(
        archive: super::ArchiveHandle,
        #[case] url: String,
        #[case] expected: &[&str],
    ) -> anyhow::Result<()> {
        let response = super::handle_has_git_repo_req(HasGitRepoRequest { url }, &archive)?;
        assert_eq!(response.status_code(), 200);
        let response_json: serde_json::Value =
            serde_json::from_reader(response.into_reader()).unwrap();

        assert_eq!(
            response_json["exists"].as_bool().unwrap(),
            !expected.is_empty()
        );

        let existing = response_json["existing"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();

        println!("existing={existing:?}\nexpected={expected:?}");

        assert!(equal_ignore_order(&existing, expected));

        Ok(())
    }

    fn equal_ignore_order(arr1: &[&str], arr2: &[&str]) -> bool {
        let set1: HashSet<_> = arr1.iter().collect();
        let set2: HashSet<_> = arr2.iter().collect();
        set1 == set2
    }

    #[rstest]
    fn handle_has_huggingface_repo_req_case_insensitive() -> anyhow::Result<()> {
        let urls = HashSet::from_iter(
            ["meta-llama/Llama-2-7b-hf", "openai/whisper-large-v3"].map(String::from),
        );

        let archive = super::HuggingfaceArchiveHandle {
            archive: Arc::new(Mutex::new(urls)),
            _watcher: None,
        };

        // Test lowercase request matches mixed case archive
        let response = super::handle_has_huggingface_repo_req(
            super::HasHuggingfaceRequest {
                repo: "meta-llama/llama-2-7b-hf".to_string(),
            },
            &archive,
        )?;
        let response_json: serde_json::Value =
            serde_json::from_reader(response.into_reader()).unwrap();
        assert_eq!(response_json["exists"].as_bool().unwrap(), true);

        // Test uppercase request matches mixed case archive
        let response = super::handle_has_huggingface_repo_req(
            super::HasHuggingfaceRequest {
                repo: "OPENAI/WHISPER-LARGE-V3".to_string(),
            },
            &archive,
        )?;
        let response_json: serde_json::Value =
            serde_json::from_reader(response.into_reader()).unwrap();
        assert_eq!(response_json["exists"].as_bool().unwrap(), true);

        // Test non-existent repo
        let response = super::handle_has_huggingface_repo_req(
            super::HasHuggingfaceRequest {
                repo: "NOT/EXISTS".to_string(),
            },
            &archive,
        )?;
        let response_json: serde_json::Value =
            serde_json::from_reader(response.into_reader()).unwrap();
        assert_eq!(response_json["exists"].as_bool().unwrap(), false);

        Ok(())
    }
}
