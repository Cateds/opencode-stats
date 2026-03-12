use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    pub fn add_assign(&mut self, other: &TokenUsage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageEvent {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub session_title: Option<String>,
    pub session_started_at: Option<DateTime<Local>>,
    pub session_archived_at: Option<DateTime<Local>>,
    pub project_name: Option<String>,
    pub project_path: Option<PathBuf>,
    pub provider_id: Option<String>,
    pub model_id: String,
    pub agent: Option<String>,
    pub finish_reason: Option<String>,
    pub tokens: TokenUsage,
    pub created_at: Option<DateTime<Local>>,
    pub completed_at: Option<DateTime<Local>>,
    pub stored_cost_usd: Option<Decimal>,
    pub source: DataSourceKind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MessageRecord {
    pub session_id: String,
    pub role: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub created_at: Option<DateTime<Local>>,
    pub source: DataSourceKind,
}

impl UsageEvent {
    pub fn pricing_model_id(&self) -> Option<String> {
        self.provider_id
            .as_deref()
            .map(|provider| format!("{provider}/{}", self.model_id))
    }

    pub fn activity_date(&self) -> Option<NaiveDate> {
        self.created_at
            .as_ref()
            .map(DateTime::date_naive)
            .or_else(|| self.session_started_at.as_ref().map(DateTime::date_naive))
    }

    pub fn duration_ms(&self) -> Option<i64> {
        let created = self.created_at?;
        let completed = self.completed_at?;
        let duration = completed.signed_duration_since(created).num_milliseconds();
        (duration > 0).then_some(duration)
    }

    pub fn is_rate_eligible(&self) -> bool {
        self.tokens.output >= 100
            && self.finish_reason.as_deref() != Some("tool-calls")
            && self.duration_ms().is_some()
    }

    pub fn project_basename(&self) -> Option<String> {
        self.project_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .or_else(|| self.project_name.clone())
    }
}

impl MessageRecord {
    pub fn activity_date(&self) -> Option<NaiveDate> {
        self.created_at.as_ref().map(DateTime::date_naive)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum DataSourceKind {
    #[default]
    Sqlite,
    Json,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub title: String,
    pub project_name: String,
    pub project_path: Option<PathBuf>,
    pub events: Vec<UsageEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

impl SessionSummary {
    pub fn from_events(session_id: String, events: Vec<UsageEvent>) -> Option<Self> {
        let first = events.first()?;
        let title = first
            .session_title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "Session {}",
                    &session_id.chars().take(8).collect::<String>()
                )
            });

        let mut project_counts: BTreeMap<String, usize> = BTreeMap::new();
        for event in &events {
            if let Some(project) = event.project_basename() {
                *project_counts.entry(project).or_default() += 1;
            }
        }
        let project_name = project_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name)
            .or_else(|| first.project_name.clone())
            .unwrap_or_else(|| "Unknown project".to_string());

        Some(Self {
            session_id,
            parent_session_id: first.parent_session_id.clone(),
            title,
            project_name,
            project_path: first.project_path.clone(),
            events,
        })
    }

    #[allow(dead_code)]
    pub fn total_tokens(&self) -> TokenUsage {
        let mut usage = TokenUsage::default();
        for event in &self.events {
            usage.add_assign(&event.tokens);
        }
        usage
    }

    #[allow(dead_code)]
    pub fn models_used(&self) -> BTreeSet<String> {
        self.events
            .iter()
            .map(|event| event.model_id.clone())
            .collect()
    }

    #[allow(dead_code)]
    pub fn interaction_count(&self) -> usize {
        self.events.len()
    }

    pub fn start_time(&self) -> Option<DateTime<Local>> {
        self.events
            .iter()
            .filter_map(|event| event.created_at)
            .min()
    }

    #[allow(dead_code)]
    pub fn end_time(&self) -> Option<DateTime<Local>> {
        self.events
            .iter()
            .filter_map(|event| event.completed_at.or(event.created_at))
            .max()
    }

    #[allow(dead_code)]
    pub fn total_duration_ms(&self) -> i64 {
        self.events.iter().filter_map(UsageEvent::duration_ms).sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppData {
    pub events: Vec<UsageEvent>,
    pub messages: Vec<MessageRecord>,
    pub session_records: Vec<SessionRecord>,
    pub import_stats: ImportStats,
    pub sessions: Vec<SessionSummary>,
    pub source: DataSourceKind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportStats {
    pub skipped_json_records: usize,
    pub skipped_sqlite_messages: usize,
}

impl ImportStats {
    pub fn summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.skipped_json_records > 0 {
            parts.push(format!(
                "Skipped {} JSON {} during import",
                self.skipped_json_records,
                pluralize(self.skipped_json_records, "record", "records")
            ));
        }
        if self.skipped_sqlite_messages > 0 {
            parts.push(format!(
                "Skipped {} SQLite {} during import",
                self.skipped_sqlite_messages,
                pluralize(self.skipped_sqlite_messages, "message", "messages")
            ));
        }

        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[cfg(test)]
mod tests {
    use super::TokenUsage;

    #[test]
    fn token_usage_saturates_instead_of_overflowing() {
        let mut usage = TokenUsage {
            input: u64::MAX,
            output: 1,
            cache_read: 0,
            cache_write: 0,
        };

        assert_eq!(usage.total(), u64::MAX);

        usage.add_assign(&TokenUsage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
        });

        assert_eq!(usage.input, u64::MAX);
        assert_eq!(usage.output, 21);
        assert_eq!(usage.cache_read, 30);
        assert_eq!(usage.cache_write, 40);
        assert_eq!(usage.total(), u64::MAX);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputOptions {
    pub database_path: Option<PathBuf>,
    pub json_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonMessageRecord {
    pub role: Option<String>,
    #[serde(rename = "providerID")]
    pub provider_id: Option<String>,
    #[serde(rename = "modelID")]
    pub model_id: Option<String>,
    pub model: Option<JsonModelRecord>,
    pub tokens: Option<JsonTokensRecord>,
    pub time: Option<JsonTimeRecord>,
    pub path: Option<JsonPathRecord>,
    pub agent: Option<String>,
    pub finish: Option<String>,
    pub cost: Option<Decimal>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonModelRecord {
    #[serde(rename = "providerID")]
    pub provider_id: Option<String>,
    #[serde(rename = "modelID")]
    pub model_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonTokensRecord {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache: Option<JsonCacheTokensRecord>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonCacheTokensRecord {
    pub read: Option<u64>,
    pub write: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonTimeRecord {
    pub created: Option<i64>,
    pub completed: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JsonPathRecord {
    pub cwd: Option<PathBuf>,
    pub root: Option<PathBuf>,
}
