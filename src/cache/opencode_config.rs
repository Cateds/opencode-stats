use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cache::errors::{Error, Result};
use crate::cache::models_cache::{map_models_root_to_local, ModelPricing};

pub fn load_pricing_overrides() -> Result<BTreeMap<String, ModelPricing>> {
    let merged = load_merged_config()?;
    Ok(extract_pricing_overrides(&merged))
}

fn load_merged_config() -> Result<Value> {
    let mut merged = Value::Object(serde_json::Map::new());
    for path in candidate_config_paths()? {
        let Some(config) = read_config_if_exists(&path)? else {
            continue;
        };
        merge_json(&mut merged, config);
    }
    Ok(merged)
}

fn candidate_config_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    if let Some(path) = dirs::config_dir() {
        paths.push(path.join("opencode").join("opencode.json"));
        paths.push(path.join("opencode").join("opencode.jsonc"));
    }

    if let Some(custom) = env::var_os("OPENCODE_CONFIG") {
        paths.push(PathBuf::from(custom));
    }

    if let Some(project) = discover_project_config_path()? {
        paths.push(project);
    }

    Ok(paths)
}

fn discover_project_config_path() -> Result<Option<PathBuf>> {
    let mut current = env::current_dir().map_err(|e| Error::CurrentDir { source: e })?;

    loop {
        for name in ["opencode.json", "opencode.jsonc"] {
            let candidate = current.join(name);
            if candidate.exists() {
                return Ok(Some(candidate));
            }
        }

        if current.join(".git").exists() {
            return Ok(None);
        }

        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        current = parent.to_path_buf();
    }
}

fn read_config_if_exists(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|e| Error::ConfigRead {
        path: path.to_path_buf(),
        source: e,
    })?;
    let value = json5::from_str::<Value>(&contents).map_err(|e| Error::ConfigParse {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(Some(value))
}

fn merge_json(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => merge_json(base_value, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}

fn extract_pricing_overrides(config: &Value) -> BTreeMap<String, ModelPricing> {
    let Some(provider_map) = config.get("provider").and_then(Value::as_object) else {
        return BTreeMap::new();
    };

    let mut result = BTreeMap::new();
    for (provider_id, provider_value) in provider_map {
        let Some(models_root) = provider_value.get("models") else {
            continue;
        };

        let models = map_models_root_to_local(provider_id, models_root);
        for (key, value) in models {
            result.insert(key, value);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{extract_pricing_overrides, merge_json};
    use rust_decimal::Decimal;
    use serde_json::json;

    #[test]
    fn merges_nested_objects() {
        let mut base = json!({
            "provider": {
                "openai": {
                    "models": {
                        "gpt-5": { "cost": { "input": 1 } }
                    }
                }
            }
        });
        merge_json(
            &mut base,
            json!({
                "provider": {
                    "openai": {
                        "models": {
                            "gpt-5": { "cost": { "output": 2 } },
                            "gpt-5-mini": { "cost": { "input": 3, "output": 4 } }
                        }
                    }
                }
            }),
        );

        let result = extract_pricing_overrides(&base);
        assert_eq!(result.get("openai/gpt-5").unwrap().input, Decimal::ONE);
        assert_eq!(
            result.get("openai/gpt-5").unwrap().output,
            Decimal::new(2, 0)
        );
        assert_eq!(
            result.get("openai/gpt-5-mini").unwrap().input,
            Decimal::new(3, 0)
        );
    }
}
