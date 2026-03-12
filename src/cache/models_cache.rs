use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::cache::http_client;
use crate::cache::opencode_config;
use crate::db::models::{TokenUsage, UsageEvent};

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const CACHE_TTL_SECS: u64 = 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelPricing {
    pub input: Decimal,
    pub output: Decimal,
    #[serde(rename = "cacheWrite", alias = "cache_write", default)]
    pub cache_write: Decimal,
    #[serde(rename = "cacheRead", alias = "cache_read", default)]
    pub cache_read: Decimal,
    #[serde(rename = "contextWindow", default)]
    pub context_window: u64,
    #[serde(rename = "sessionQuota", default)]
    pub session_quota: Decimal,
}

impl ModelPricing {
    pub fn with_fallbacks(mut self) -> Self {
        if self.cache_write.is_zero() {
            self.cache_write = self.input;
        }
        if self.cache_read.is_zero() {
            self.cache_read = self.input * Decimal::new(1, 1);
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct PricingCatalog {
    pub models: BTreeMap<String, ModelPricing>,
    pub cache_path: PathBuf,
    pub refresh_needed: bool,
}

impl PricingCatalog {
    pub fn load() -> Result<Self> {
        let cache_path = default_cache_path()?;
        let cached = load_cached_models(&cache_path).unwrap_or_default();
        let config = opencode_config::load_pricing_overrides()?;
        let refresh_needed = cache_is_stale(&cache_path).unwrap_or(true);
        let merged = merge_with_priority(cached, config);

        Ok(Self {
            models: merged,
            cache_path,
            refresh_needed,
        })
    }

    pub fn lookup(&self, model_id: &str) -> Option<&ModelPricing> {
        lookup_model(&self.models, model_id)
    }

    pub fn lookup_for_event(&self, event: &UsageEvent) -> Option<&ModelPricing> {
        if let Some(key) = event.pricing_model_id() {
            return lookup_exact_model(&self.models, &key);
        }

        if event.provider_id.is_none() {
            return self.lookup(&event.model_id);
        }

        None
    }

    pub fn cost_for_event(&self, event: &UsageEvent) -> Decimal {
        if let Some(cost) = event.stored_cost_usd {
            return cost;
        }

        let Some(pricing) = self.lookup_for_event(event) else {
            return Decimal::ZERO;
        };
        price_tokens(&event.tokens, pricing)
    }

    pub fn has_pricing_for_event(&self, event: &UsageEvent) -> bool {
        self.lookup_for_event(event).is_some()
    }

    fn from_sources(cache_path: PathBuf, remote: BTreeMap<String, ModelPricing>) -> Result<Self> {
        let config = opencode_config::load_pricing_overrides()?;
        let merged = merge_with_priority(remote, config);
        Ok(Self {
            models: merged,
            cache_path,
            refresh_needed: false,
        })
    }
}

pub fn price_tokens(tokens: &TokenUsage, pricing: &ModelPricing) -> Decimal {
    let million = Decimal::from(1_000_000u64);
    (Decimal::from(tokens.input) * pricing.input
        + Decimal::from(tokens.output) * pricing.output
        + Decimal::from(tokens.cache_write) * pricing.cache_write
        + Decimal::from(tokens.cache_read) * pricing.cache_read)
        / million
}

pub fn default_cache_path() -> Result<PathBuf> {
    let Some(config_dir) = dirs::config_dir() else {
        anyhow::bail!("could not determine config directory");
    };
    Ok(config_dir.join("oc-stats").join("models.json"))
}

pub async fn refresh_remote_models(
    cache_path: PathBuf,
    sender: mpsc::UnboundedSender<Result<PricingCatalog>>,
) {
    let fetch_result = fetch_remote_catalog(&cache_path).await;
    let _ = sender.send(fetch_result);
}

async fn fetch_remote_catalog(cache_path: &Path) -> Result<PricingCatalog> {
    let payload = http_client::fetch_json(MODELS_DEV_URL).await?;
    let remote = map_models_root_to_local("", &payload);
    persist_cached_models(cache_path, &remote)?;
    PricingCatalog::from_sources(cache_path.to_path_buf(), remote)
}

fn persist_cached_models(path: &Path, models: &BTreeMap<String, ModelPricing>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache dir {}", parent.display()))?;
    }
    let temp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(models).context("failed to encode cached pricing")?;
    fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("failed to move {} into place", temp.display()))?;
    Ok(())
}

fn load_cached_models(path: &Path) -> Result<BTreeMap<String, ModelPricing>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let raw = serde_json::from_slice::<BTreeMap<String, ModelPricing>>(&bytes)
        .context("failed to parse cached models")?;
    Ok(raw
        .into_iter()
        .map(|(key, value)| (key.to_lowercase(), value.with_fallbacks()))
        .collect())
}

fn cache_is_stale(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::from_secs(CACHE_TTL_SECS + 1));
    Ok(age.as_secs() >= CACHE_TTL_SECS)
}

fn merge_with_priority(
    lower: BTreeMap<String, ModelPricing>,
    higher: BTreeMap<String, ModelPricing>,
) -> BTreeMap<String, ModelPricing> {
    let mut merged = lower;
    for (key, value) in higher {
        merged.insert(key, value.with_fallbacks());
    }
    merged
}

fn lookup_model<'a>(
    models: &'a BTreeMap<String, ModelPricing>,
    model_id: &str,
) -> Option<&'a ModelPricing> {
    let lowercase = model_id.to_lowercase();
    if let Some(value) = models.get(&lowercase) {
        return Some(value);
    }

    let normalized = normalize_model_key(model_id);
    if let Some(value) = models.get(&normalized) {
        return Some(value);
    }

    if let Some((_, bare)) = lowercase.split_once('/') {
        let normalized_bare = normalize_model_key(bare);
        if let Some(value) = models.get(&normalized_bare) {
            return Some(value);
        }
    }

    None
}

fn lookup_exact_model<'a>(
    models: &'a BTreeMap<String, ModelPricing>,
    model_id: &str,
) -> Option<&'a ModelPricing> {
    let lowercase = model_id.to_lowercase();
    if let Some(value) = models.get(&lowercase) {
        return Some(value);
    }

    let normalized = normalize_model_key(model_id);
    models.get(&normalized)
}

pub(crate) fn map_models_root_to_local(
    default_provider: &str,
    payload: &serde_json::Value,
) -> BTreeMap<String, ModelPricing> {
    let mut result = BTreeMap::new();

    if let Some(providers) = payload.get("providers").and_then(|value| value.as_object()) {
        for (provider_id, provider_data) in providers {
            collect_provider_models(&mut result, provider_id, provider_data.get("models"));
        }
        return result;
    }

    if let Some(root) = payload.as_object() {
        if !default_provider.is_empty() {
            collect_provider_models(
                &mut result,
                default_provider,
                payload.get("models").or(Some(payload)),
            );
            return result;
        }

        if root.values().any(|value| value.get("models").is_some()) {
            for (provider_id, provider_data) in root {
                collect_provider_models(&mut result, provider_id, provider_data.get("models"));
            }
            return result;
        }
    }

    collect_provider_models(
        &mut result,
        default_provider,
        payload.get("models").or(Some(payload)),
    );
    result
}

fn collect_provider_models(
    result: &mut BTreeMap<String, ModelPricing>,
    provider_id: &str,
    models_root: Option<&serde_json::Value>,
) {
    let Some(models) = models_root.and_then(|value| value.as_object()) else {
        return;
    };

    for (model_id, model_data) in models {
        let Some(pricing) = pricing_from_model(model_data) else {
            continue;
        };

        let bare = normalize_model_key(model_id);
        let provider = provider_id.to_lowercase();
        let key = if provider.is_empty() {
            bare
        } else {
            format!("{provider}/{bare}")
        };
        result.insert(key, pricing);
    }
}

fn pricing_from_model(model_data: &serde_json::Value) -> Option<ModelPricing> {
    let cost = model_data.get("cost").and_then(|value| value.as_object())?;
    let limit = model_data.get("limit").and_then(|value| value.as_object());

    Some(
        ModelPricing {
            input: decimal_from_json(
                cost.get("input")
                    .or_else(|| cost.get("prompt"))
                    .or_else(|| cost.get("prompt_text")),
            ),
            output: decimal_from_json(
                cost.get("output")
                    .or_else(|| cost.get("completion"))
                    .or_else(|| cost.get("completion_text")),
            ),
            cache_write: decimal_from_json(
                cost.get("cache_write")
                    .or_else(|| cost.get("input_cache_write"))
                    .or_else(|| cost.get("write")),
            ),
            cache_read: decimal_from_json(
                cost.get("cache_read")
                    .or_else(|| cost.get("input_cache_read"))
                    .or_else(|| cost.get("read")),
            ),
            context_window: limit
                .and_then(|map| map.get("context"))
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
            session_quota: Decimal::ZERO,
        }
        .with_fallbacks(),
    )
}

fn decimal_from_json(value: Option<&serde_json::Value>) -> Decimal {
    let Some(value) = value else {
        return Decimal::ZERO;
    };

    if let Some(number) = value.as_f64() {
        Decimal::try_from(number).unwrap_or(Decimal::ZERO)
    } else if let Some(number) = value.as_str() {
        number.parse::<Decimal>().unwrap_or(Decimal::ZERO)
    } else {
        Decimal::ZERO
    }
}

pub fn normalize_model_key(model_id: &str) -> String {
    let mut model = model_id.to_lowercase();
    if let Some((provider, bare)) = model.split_once('/') {
        let normalized_bare = normalize_model_key(bare);
        return format!("{provider}/{normalized_bare}");
    }

    if model.len() > 9 {
        let suffix = &model[model.len() - 9..];
        if suffix.starts_with('-') && suffix[1..].chars().all(|value| value.is_ascii_digit()) {
            model.truncate(model.len() - 9);
        }
    }

    model = regexless_replace_version(&model, "claude-opus-");
    model = regexless_replace_version(&model, "claude-sonnet-");
    model = regexless_replace_version(&model, "claude-haiku-");
    model = regexless_replace_version(&model, "gpt-");

    if let Some(stripped) = model.strip_prefix("kimi-k-")
        && stripped
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
    {
        model = format!("kimi-k{stripped}");
    }

    model
}

fn regexless_replace_version(value: &str, prefix: &str) -> String {
    if let Some(rest) = value.strip_prefix(prefix) {
        let segments: Vec<&str> = rest.split('-').collect();
        if segments.len() >= 2
            && segments[0].chars().all(|ch| ch.is_ascii_digit())
            && segments[1].chars().all(|ch| ch.is_ascii_digit())
        {
            let merged = format!("{}.{}", segments[0], segments[1]);
            let suffix = if segments.len() > 2 {
                format!("-{}", segments[2..].join("-"))
            } else {
                String::new()
            };
            return format!("{prefix}{merged}{suffix}");
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::{ModelPricing, map_models_root_to_local, normalize_model_key};
    use crate::db::models::{DataSourceKind, TokenUsage, UsageEvent};
    use rust_decimal::Decimal;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn normalizes_date_suffixes() {
        assert_eq!(
            normalize_model_key("claude-sonnet-4-5-20250514"),
            "claude-sonnet-4.5"
        );
        assert_eq!(normalize_model_key("gpt-5-1"), "gpt-5.1");
    }

    #[test]
    fn fills_cache_fallbacks() {
        let pricing = ModelPricing {
            input: Decimal::new(3, 0),
            output: Decimal::new(15, 0),
            cache_write: Decimal::ZERO,
            cache_read: Decimal::ZERO,
            context_window: 0,
            session_quota: Decimal::ZERO,
        }
        .with_fallbacks();

        assert_eq!(pricing.cache_write, Decimal::new(3, 0));
        assert_eq!(pricing.cache_read, Decimal::new(3, 0) * Decimal::new(1, 1));
    }

    #[test]
    fn maps_models_dev_root() {
        let mapped = map_models_root_to_local(
            "",
            &json!({
                "openai": {
                    "id": "openai",
                    "models": {
                        "gpt-5": {
                            "cost": { "input": 1, "output": 2, "cache_read": 0.1, "cache_write": 0.2 },
                            "limit": { "context": 1000, "output": 100 }
                        }
                    }
                }
            }),
        );

        assert_eq!(mapped.get("openai/gpt-5").unwrap().input, Decimal::ONE);
    }

    #[test]
    fn provider_lookup_does_not_fall_back_to_bare_model() {
        let mut models = BTreeMap::new();
        models.insert(
            "anthropic/claude-sonnet-4.5".to_string(),
            ModelPricing {
                input: Decimal::ONE,
                output: Decimal::new(2, 0),
                cache_write: Decimal::ONE,
                cache_read: Decimal::new(1, 1),
                context_window: 0,
                session_quota: Decimal::ZERO,
            },
        );
        let catalog = super::PricingCatalog {
            models,
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
        };
        let event = UsageEvent {
            session_id: "ses".to_string(),
            parent_session_id: None,
            session_title: None,
            session_started_at: None,
            session_archived_at: None,
            project_name: None,
            project_path: None,
            provider_id: Some("openai".to_string()),
            model_id: "claude-sonnet-4.5".to_string(),
            agent: None,
            finish_reason: None,
            tokens: TokenUsage::default(),
            created_at: None,
            completed_at: None,
            stored_cost_usd: None,
            source: DataSourceKind::Json,
        };

        assert!(catalog.lookup_for_event(&event).is_none());
    }
}
