use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::StructOpt;
use journal::{run, Cli, Config, WallClock};
use tracing::Level;

fn to_level<S: AsRef<str>>(level: S) -> Result<Level, ()> {
    Level::from_str(level.as_ref()).map_err(|_| ())
}

fn init_logs() {
    let level = std::env::var("JOURNAL__LOG_LEVEL")
        .map_err(|_| ())
        .and_then(to_level)
        .unwrap_or(Level::ERROR);

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(level)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_logs();

    let cli = Cli::parse();
    let config = Config::load().context("Failed to load configuration")?;

    let clock = WallClock;
    let open = |path: &Path| open::that(path).map_err(|e| anyhow::anyhow!(e));

    run(cli, &config, &clock, open).await
}
