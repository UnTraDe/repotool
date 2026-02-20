use clap::{Args, Subcommand, ValueEnum};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::{fs, path::PathBuf};

use crate::git_url;

#[derive(Subcommand, Debug, Clone)]
pub enum Platform {
    Github {
        #[arg(value_enum)]
        group_type: RepositoryGroupType,

        input: String,
    },
    Gitlab {
        #[arg(value_enum)]
        group_type: RepositoryGroupType,

        /// GitLab instance URL (e.g., https://gitlab.com or https://gitlab.archlinux.org)
        #[arg(short, long)]
        instance: String,

        /// Group or user name
        input: String,
    },
}

#[derive(ValueEnum, Debug, Clone)]
pub enum RepositoryGroupType {
    Org,
    User,
}

#[derive(Args, Debug)]
pub struct CloneParams {
    /// Repository type
    #[command(subcommand)]
    platform: Platform,

    /// Compare repository list with a given file, and only clone the ones that are not in the list
    #[arg(short, long)]
    compare_file: Option<PathBuf>,

    /// Filter out forks
    #[arg(long, group = "forks")]
    filter_forks: bool,

    /// Only clone forks
    #[arg(long, group = "forks")]
    only_forks: bool,

    /// Include submodules
    #[arg(long)]
    include_submodules: bool,

    /// Output repository list to a file instead of cloning
    #[arg(short, long)]
    output_file: Option<PathBuf>,

    /// Prepand something to each repository in the output file
    #[arg(
        short,
        long,
        default_value = "git clone --mirror",
        requires = "output_file"
    )]
    prepand_command: String,
}

pub struct Entry {
    pub clone_url: String,
    pub is_fork: bool,
}

pub fn clone(params: CloneParams) -> anyhow::Result<()> {
    if params.include_submodules {
        unimplemented!("submodules are not yet supported");
    }

    let compare = if let Some(compare_file) = params.compare_file {
        HashSet::from_iter(
            io::BufReader::new(fs::File::open(compare_file)?)
                .lines()
                .collect::<io::Result<Vec<String>>>()?,
        )
    } else {
        HashSet::new()
    };

    let repos = match params.platform {
        Platform::Github { group_type, input } => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(github(group_type, &input))?
        }
        Platform::Gitlab {
            group_type,
            instance,
            input,
        } => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(gitlab(group_type, &instance, &input))?
        }
    }
    .into_iter()
    .filter(|e| {
        if params.filter_forks {
            !e.is_fork
        } else if params.only_forks {
            e.is_fork
        } else {
            true
        }
    })
    .collect::<Vec<Entry>>();

    let total_repo_count = repos.len();

    let repos = repos
        .into_iter()
        .filter(|e| !git_url::is_in_compare_list(&e.clone_url, &compare))
        .collect::<Vec<Entry>>();

    log::info!(
        "got {total_repo_count} repositories, skipped {} ",
        total_repo_count - repos.len()
    );

    if let Some(output) = &params.output_file {
        let mut output = io::BufWriter::new(
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output)?,
        );

        for r in repos {
            writeln!(output, "{} {}", params.prepand_command, r.clone_url)?;
        }
    }

    Ok(())
}

async fn github(group_type: RepositoryGroupType, name: &str) -> anyhow::Result<Vec<Entry>> {
    match group_type {
        RepositoryGroupType::Org => fetch_github_org_repos(name).await,
        RepositoryGroupType::User => fetch_github_user_repos(name).await,
    }
}

/// Fetch all repositories from a GitHub organization
pub async fn fetch_github_org_repos(name: &str) -> anyhow::Result<Vec<Entry>> {
    let octocrab = octocrab::instance();

    log::info!("fetching page 1...");
    let page = octocrab
        .orgs(name)
        .list_repos()
        .per_page(100)
        .send()
        .await?;

    let pages = page.number_of_pages().unwrap_or(1);
    log::info!("total pages: {pages}");
    let mut current_page = 1;

    let mut repos = page.items;

    while current_page < pages {
        current_page += 1;
        log::info!("fetching page {}...", current_page);
        repos.append(
            &mut octocrab
                .orgs(name)
                .list_repos()
                .per_page(100)
                .page(current_page)
                .send()
                .await?
                .items,
        );
    }

    Ok(repos
        .into_iter()
        .filter_map(|r| match (r.clone_url, r.fork) {
            (Some(url), Some(fork)) => Some(Entry {
                clone_url: url.as_str().to_owned(),
                is_fork: fork,
            }),
            (u, f) => {
                log::error!(
                    "'{}': expected fields to be present, but instead clone_url = {u:?}, fork = {f:?}",
                    r.name
                );
                None
            }
        })
        .collect())
}

/// Fetch all repositories from a GitHub user
pub async fn fetch_github_user_repos(name: &str) -> anyhow::Result<Vec<Entry>> {
    let octocrab = octocrab::instance();

    let mut repos: Vec<octocrab::models::Repository> = Vec::new();
    let mut current_page = 1u32;

    loop {
        log::info!("fetching page {}...", current_page);
        let page: Vec<octocrab::models::Repository> = octocrab
            .get(
                format!("/users/{name}/repos"),
                Some(&[("per_page", "100"), ("page", &current_page.to_string())]),
            )
            .await?;

        if page.is_empty() {
            break;
        }

        repos.extend(page);
        current_page += 1;
    }

    log::info!("fetched {} repositories total", repos.len());

    Ok(repos
        .into_iter()
        .filter_map(|r| match (r.clone_url, r.fork) {
            (Some(url), Some(fork)) => Some(Entry {
                clone_url: url.as_str().to_owned(),
                is_fork: fork,
            }),
            (u, f) => {
                log::error!(
                    "'{}': expected fields to be present, but instead clone_url = {u:?}, fork = {f:?}",
                    r.name
                );
                None
            }
        })
        .collect())
}

/// Filter entries by fork status
pub fn filter_by_forks(entries: Vec<Entry>, filter_forks: bool, only_forks: bool) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| {
            if filter_forks {
                !e.is_fork
            } else if only_forks {
                e.is_fork
            } else {
                true
            }
        })
        .collect()
}

async fn gitlab(
    group_type: RepositoryGroupType,
    instance: &str,
    name: &str,
) -> anyhow::Result<Vec<Entry>> {
    use gitlab::api::{groups::projects::GroupProjects, ApiError, AsyncQuery};
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct GitLabProject {
        http_url_to_repo: Option<String>,
        forked_from_project: Option<serde_json::Value>,
    }

    let client = gitlab::GitlabBuilder::new(instance, "")
        .build_async()
        .await?;

    Ok(match group_type {
        RepositoryGroupType::Org => {
            log::info!("fetching projects from GitLab group '{}'...", name);

            let endpoint = GroupProjects::builder()
                .group(name)
                .build()
                .map_err(|e| anyhow::anyhow!("failed to build GitLab query: {}", e))?;

            // Query all results using the pager
            let repos: Vec<GitLabProject> =
                gitlab::api::paged(endpoint, gitlab::api::Pagination::All)
                    .query_async(&client)
                    .await
                    .map_err(|e: gitlab::api::ApiError<_>| match e {
                        ApiError::GitlabService { status, .. } if status.as_u16() == 404 => {
                            anyhow::anyhow!("Group '{}' not found", name)
                        }
                        e => anyhow::anyhow!("GitLab API error: {}", e),
                    })?;

            log::info!("fetched {} projects total", repos.len());
            repos
        }
        RepositoryGroupType::User => {
            anyhow::bail!("User repositories are not yet supported for GitLab")
        }
    }
    .into_iter()
    .filter_map(|r| {
        let clone_url = r.http_url_to_repo?;
        let is_fork = r.forked_from_project.is_some();

        Some(Entry { clone_url, is_fork })
    })
    .collect())
}
