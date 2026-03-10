mod analytics;
mod cache;
mod db;
mod ui;
mod utils;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cache::models_cache::PricingCatalog;
use crate::db::models::InputOptions;
use crate::db::queries::load_app_data;
use crate::ui::app::App;
use crate::ui::theme::ThemeMode;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = CliArgs::parse();
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
    #[arg(long = "db", value_name = "PATH")]
    database_path: Option<PathBuf>,

    #[arg(long = "json", value_name = "PATH")]
    json_path: Option<PathBuf>,

    #[arg(long = "theme", default_value = "dark")]
    theme: ThemeMode,
}

