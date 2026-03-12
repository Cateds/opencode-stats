mod analytics;
mod cache;
mod db;
mod ui;
mod utils;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::cache::models_cache::{PricingCatalog, default_cache_path, refresh_remote_models};
use crate::db::models::InputOptions;
use crate::db::queries::load_app_data;
use crate::ui::app::App;
use crate::ui::theme::ThemeMode;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = CliArgs::parse();
    if let Some(command) = cli.command {
        return run_cache_command(command).await;
    }

    let data = load_app_data(&InputOptions {
        database_path: cli.database_path,
        json_path: cli.json_path,
    })
    .context("failed to load OpenCode usage data")?;

    let pricing = PricingCatalog::load().context("failed to load pricing catalog")?;
    let app = App::new(data, pricing, cli.theme);
    app.run()
}

#[derive(Debug, Parser)]
#[command(name = "oc-stats")]
#[command(version, about)]
struct CliArgs {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long = "db", value_name = "PATH")]
    database_path: Option<PathBuf>,

    #[arg(long = "json", value_name = "PATH")]
    json_path: Option<PathBuf>,

    #[arg(long = "theme", default_value = "dark")]
    theme: ThemeMode,
}

#[derive(Debug, Subcommand)]
enum Command {
    Cache {
        #[command(subcommand)]
        action: CacheCommand,
    },
}

#[derive(Debug, Subcommand)]
#[command(about = "Manage the local cache of model pricing data")]
enum CacheCommand {
    #[command(about = "Show the path to the local pricing cache file")]
    Path,
    #[command(about = "Update the local pricing cache")]
    Update,
    #[command(about = "Clean the local pricing cache")]
    Clean,
}

async fn run_cache_command(command: Command) -> Result<()> {
    match command {
        Command::Cache { action } => match action {
            CacheCommand::Path => {
                println!("{}", default_cache_path()?.display());
                Ok(())
            }
            CacheCommand::Update => {
                let path = default_cache_path()?;
                let (sender, mut receiver) = mpsc::unbounded_channel();
                refresh_remote_models(path.clone(), sender).await;
                let result = receiver.try_recv();
                if let Ok(Ok(_)) = result {
                    println!("Updated {}", path.display());
                } else {
                    println!("Failed to update {}", path.display());
                }
                Ok(())
            }
            CacheCommand::Clean => {
                let path = default_cache_path()?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("failed to remove {}", path.display()))?;
                }
                println!("Cleaned {}", path.display());
                Ok(())
            }
        },
    }
}
