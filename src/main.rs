mod config;
mod mesh;
mod protocol;
mod storage;
mod web;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[arg(short, long, default_value = "config/example.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = config::AppConfig::load(&cli.config).await?;
    let runtime = mesh::Runtime::new(config).await?;
    runtime.clone().start().await?;
    web::serve(runtime).await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,canweeb=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).compact().init();
}
