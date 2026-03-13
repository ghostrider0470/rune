//! Thin CLI binary — parses args and delegates to `rune-cli`.

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = rune_cli::Cli::parse();
    rune_cli::run(cli).await
}
