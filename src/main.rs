use clap::{Parser, Subcommand, ValueEnum};

mod clone;
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

    Hash(hash::HashParams),
    Serve(serve::ServeParams),
}

#[derive(Subcommand, Debug, Clone)]
enum Platform {
    Github {
        #[arg(value_enum)]
        group_type: RepositoryGroupType,

        input: String,
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
        Commands::Hash(hash) => hash::run(hash),
        Commands::Serve(serve) => serve::run(serve),
    }
}
