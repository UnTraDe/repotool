use clap::Args;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::{fs, path::PathBuf};

#[derive(Args, Debug)]
pub struct CloneParams {
    /// Repository type
    #[command(subcommand)]
    platform: crate::Platform,

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

struct Entry {
    clone_url: String,
    is_fork: bool,
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
        crate::Platform::Github { group_type, input } => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(github(group_type, &input))?
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
        .filter(|e| !is_in_compare_list(&e.clone_url, &compare))
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

async fn github(group_type: crate::RepositoryGroupType, name: &str) -> anyhow::Result<Vec<Entry>> {
    let octocrab = octocrab::instance();

    Ok(match group_type {
        crate::RepositoryGroupType::Org => {
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

            repos
        }
        crate::RepositoryGroupType::User => todo!(), // octocrab.users(name).repos().send().await?,
    }
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

fn is_in_compare_list(url: &str, compare: &HashSet<String>) -> bool {
    if compare.contains(url) {
        return true;
    }

    if let Some(url) = url.strip_suffix(".git") {
        if compare.contains(url) {
            return true;
        }
    } else if compare.contains(&format!("{}.git", url)) {
        return true;
    }

    // TODO(tomer) compare across different schemes as well?

    false
}
