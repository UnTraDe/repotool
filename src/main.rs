use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

mod clone;
mod scan;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Verbose printing
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Debug)]
struct CloneParams {
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

    /// Prepand command
    #[arg(short, long, default_value = "git clone --mirror")]
    prepand_command: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan local filesystem for repositories
    Scan(scan::ScanParams),

    /// Clone repositories
    Clone(CloneParams),
}

#[derive(Subcommand, Debug)]
enum Platform {
    Github {
        #[arg(value_enum)]
        list: RepositoryGroupType,
    },
}

#[derive(ValueEnum, Debug, Clone)]
enum RepositoryGroupType {
    Org,
    User,
}

fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    pretty_env_logger::init();
    let cli = Cli::parse();
    log::trace!("cli {cli:?}");

    match cli.command {
        Commands::Scan(params) => scan::scan(params),
        Commands::Clone(params) => clone::clone(params),
    }
}
