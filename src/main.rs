use clap::Parser;
use graphswarm::cli::{Cli, Commands};
use graphswarm::utils::setup_logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    setup_logging("info");

    match cli.command {
        Commands::Index(cmd) => cmd.execute().await?,
        Commands::Query(cmd) => cmd.execute().await?,
        Commands::Server(cmd) => cmd.execute().await?,
    }

    Ok(())
}
