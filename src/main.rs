mod analytics;
mod cache;
mod db;
mod ui;
mod utils;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::cache::models_cache::{PricingCatalog, default_cache_path, refresh_pricing_catalog};
use crate::db::models::InputOptions;
use crate::db::queries::load_app_data;
use crate::ui::app::App;
use crate::ui::theme::ThemeMode;

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
    app.run().await
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
                let current = PricingCatalog::load().ok();
                let message = finalize_cache_update(
                    &path,
                    current.as_ref(),
                    refresh_pricing_catalog(path.clone()).await,
                )?;
                println!("{message}");
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

fn finalize_cache_update(
    path: &std::path::Path,
    current: Option<&PricingCatalog>,
    result: Result<PricingCatalog>,
) -> Result<String> {
    match result {
        Ok(_) => Ok(format!("Updated {}", path.display())),
        Err(err) => {
            let fallback_hint = current
                .map(PricingCatalog::refresh_failure_hint)
                .unwrap_or("current pricing fallback status is unknown");
            Err(err.context(format!(
                "failed to update {}; {fallback_hint}",
                path.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::finalize_cache_update;
    use crate::cache::models_cache::{PricingAvailability, PricingCatalog};
    use anyhow::{Result, anyhow};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn test_catalog(availability: PricingAvailability) -> PricingCatalog {
        PricingCatalog {
            models: BTreeMap::new(),
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability,
            load_notice: None,
        }
    }

    #[test]
    fn cache_update_success_keeps_success_message() {
        let path = Path::new("/tmp/models.json");
        let result = finalize_cache_update(
            path,
            None,
            Ok::<PricingCatalog, _>(test_catalog(PricingAvailability::Cached)),
        )
        .unwrap();

        assert_eq!(result, "Updated /tmp/models.json");
    }

    #[test]
    fn cache_update_failure_returns_error_with_fallback_hint() {
        let path = Path::new("/tmp/models.json");
        let err = finalize_cache_update(
            path,
            Some(&test_catalog(PricingAvailability::OverridesOnly)),
            Err(anyhow!("network down")),
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("failed to update /tmp/models.json"));
        assert!(message.contains("using local pricing overrides only"));
    }

    #[test]
    fn cache_update_failure_without_catalog_still_returns_error() {
        let path = Path::new("/tmp/models.json");
        let result: Result<PricingCatalog> = Err(anyhow!("network down"));
        let err = finalize_cache_update(path, None, result).unwrap_err();

        assert!(format!("{err:#}").contains("current pricing fallback status is unknown"));
    }
}
