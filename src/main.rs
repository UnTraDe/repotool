use clap::{Parser, Subcommand};

mod archive;
mod clone;
mod fetch;
mod fsck;
mod git_url;
mod grab;
mod hash;
mod scan;
mod serve;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Verbose printing
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan local filesystem for repositories
    Scan(scan::ScanParams),

    /// Clone repositories
    Clone(clone::CloneParams),

    /// Grab repositories (clone and add to archive)
    Grab(grab::GrabParams),

    /// Fetch all remotes in mirror repositories
    Fetch(fetch::FetchParams),

    /// Run git fsck on bare repositories
    Fsck(fsck::FsckParams),

    Hash(hash::HashParams),
    Serve(serve::ServeParams),
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();
    let cli = Cli::parse();
    log::trace!("cli {cli:?}");

    match cli.command {
        Commands::Scan(params) => scan::scan(params),
        Commands::Clone(params) => clone::clone(params),
        Commands::Fetch(params) => fetch::run(params),
        Commands::Fsck(params) => fsck::run(params),
        Commands::Grab(params) => grab::run(params),
        Commands::Hash(hash) => hash::run(hash),
        Commands::Serve(serve) => serve::run(serve),
    }
}
