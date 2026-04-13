pub mod daily;
pub mod heatmap_data;
pub mod model_stats;
pub mod monthly;
pub mod weekly;

use std::collections::BTreeSet;

use chrono::NaiveDate;

use crate::analytics::daily::aggregate_daily;
use crate::analytics::heatmap_data::{HeatmapData, build_heatmap_data};
use crate::analytics::model_stats::{
    ModelChartData, ModelUsageRow, ProviderUsageRow, build_model_chart, build_provider_chart,
};
use crate::analytics::monthly::aggregate_monthly;
use crate::analytics::weekly::aggregate_weekly;
use crate::cache::models_cache::PricingCatalog;
use crate::db::models::{AppData, MessageRecord, UsageEvent};
use crate::utils::pricing::{PriceSummary, ZeroCostBehavior, update_price_summary};
use crate::utils::time::{TimeRange, current_local_date};

#[derive(Clone, Debug)]
pub struct OverviewStats {
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub total_cost: PriceSummary,
    pub sessions: usize,
    pub messages: usize,
    pub prompts: usize,
    pub models_used: usize,
    pub active_days: usize,
    pub fun_comparison: String,
}

#[derive(Clone, Debug)]
pub struct AnalyticsSnapshot {
    pub overview: OverviewStats,
    pub models: Vec<ModelUsageRow>,
    pub chart: ModelChartData,
    pub providers: Vec<ProviderUsageRow>,
    pub provider_chart: ModelChartData,
    pub heatmap: HeatmapData,
}

pub fn build_snapshot(
    data: &AppData,
    pricing: &PricingCatalog,
    range: TimeRange,
    zero_cost_behavior: ZeroCostBehavior,
) -> AnalyticsSnapshot {
    let today = current_local_date();
    let filtered_events = filter_events(&data.events, range, today);
    let filtered_messages = filter_messages(&data.messages, range, today);
    let filtered_model_messages = filtered_messages
        .iter()
        .filter(|message| message.model_id.is_some())
        .cloned()
        .collect::<Vec<_>>();
    let daily = aggregate_daily(&filtered_events, pricing, today, zero_cost_behavior);
    let weekly = aggregate_weekly(&daily, 0);
    let _monthly = aggregate_monthly(&weekly);
    let (models, chart) = build_model_chart(
        &filtered_events,
        &filtered_model_messages,
        pricing,
        range,
        today,
        zero_cost_behavior,
    );
    let (providers, provider_chart) = build_provider_chart(
        &filtered_events,
        &filtered_model_messages,
        pricing,
        range,
        today,
        zero_cost_behavior,
    );
    let heatmap = build_heatmap_data(&data.events, today);

    let total_tokens = saturating_sum(filtered_events.iter().map(|event| event.tokens.total()));
    let input_tokens = saturating_sum(filtered_events.iter().map(|event| event.tokens.input));
    let output_tokens = saturating_sum(filtered_events.iter().map(|event| event.tokens.output));
    let cache_tokens = saturating_sum(filtered_events.iter().map(|event| {
        event
            .tokens
            .cache_read
            .saturating_add(event.tokens.cache_write)
    }));
    let mut total_cost = PriceSummary::default();
    for event in &filtered_events {
        update_price_summary(&mut total_cost, pricing, event, zero_cost_behavior);
    }
    let session_ids = filter_session_ids(data, &filtered_events, &filtered_messages, range, today);
    let messages = data
        .messages
        .iter()
        .filter(|message| session_ids.contains(&message.session_id))
        .count();
    let prompts = data
        .messages
        .iter()
        .filter(|message| {
            session_ids.contains(&message.session_id) && message.role.as_deref() == Some("user")
        })
        .count();
    let sessions = session_ids.len();
    let models_used = filtered_events
        .iter()
        .map(|event| event.model_id.clone())
        .collect::<BTreeSet<_>>()
        .len();
    let active_days = daily.len();

    AnalyticsSnapshot {
        overview: OverviewStats {
            total_tokens,
            input_tokens,
            output_tokens,
            cache_tokens,
            total_cost,
            sessions,
            messages,
            prompts,
            models_used,
            active_days,
            fun_comparison: crate::utils::formatting::tokens_comparison_text(total_tokens),
        },
        models,
        chart,
        providers,
        provider_chart,
        heatmap,
    }
}

fn saturating_sum(values: impl IntoIterator<Item = u64>) -> u64 {
    values
        .into_iter()
        .fold(0u64, |total, value| total.saturating_add(value))
}

fn filter_events(events: &[UsageEvent], range: TimeRange, today: NaiveDate) -> Vec<UsageEvent> {
    events
        .iter()
        .filter(|event| {
            event
                .activity_date()
                .is_some_and(|date| crate::utils::time::in_range(date, range, today))
        })
        .cloned()
        .collect()
}

fn filter_messages(
    messages: &[MessageRecord],
    range: TimeRange,
    today: NaiveDate,
) -> Vec<MessageRecord> {
    messages
        .iter()
        .filter(|message| {
            message
                .activity_date()
                .is_some_and(|date| crate::utils::time::in_range(date, range, today))
        })
        .cloned()
        .collect()
}

fn filter_session_ids(
    data: &AppData,
    filtered_events: &[UsageEvent],
    filtered_messages: &[MessageRecord],
    range: TimeRange,
    today: NaiveDate,
) -> BTreeSet<String> {
    let session_ids = data
        .session_records
        .iter()
        .filter(|session| {
            crate::utils::time::in_range(session.updated_at.date_naive(), range, today)
        })
        .map(|session| session.session_id.clone())
        .collect::<BTreeSet<_>>();

    if !session_ids.is_empty() {
        return session_ids;
    }

    filtered_events
        .iter()
        .map(|event| event.session_id.clone())
        .chain(
            filtered_messages
                .iter()
                .map(|message| message.session_id.clone()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::build_snapshot;
    use crate::cache::models_cache::{PricingAvailability, PricingCatalog};
    use crate::db::models::{
        AppData, DataSourceKind, ImportStats, MessageRecord, SessionRecord, TokenUsage, UsageEvent,
    };
    use crate::utils::pricing::ZeroCostBehavior;
    use crate::utils::time::TimeRange;
    use chrono::{Local, TimeZone};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn overview_counts_match_even_without_session_records() {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let updated_at = Local
            .with_ymd_and_hms(2026, 3, 12, 10, 0, 0)
            .single()
            .unwrap();

        let messages = vec![
            MessageRecord {
                session_id: "ses_1".to_string(),
                role: Some("user".to_string()),
                provider_id: None,
                model_id: None,
                created_at: Some(created_at),
                source: DataSourceKind::Json,
            },
            MessageRecord {
                session_id: "ses_1".to_string(),
                role: Some("assistant".to_string()),
                provider_id: Some("openai".to_string()),
                model_id: Some("gpt-5".to_string()),
                created_at: Some(updated_at),
                source: DataSourceKind::Json,
            },
        ];

        let events = vec![UsageEvent {
            session_id: "ses_1".to_string(),
            parent_session_id: None,
            session_title: Some("Session 1".to_string()),
            session_started_at: Some(created_at),
            session_archived_at: None,
            project_name: Some("demo".to_string()),
            project_path: None,
            provider_id: Some("openai".to_string()),
            model_id: "gpt-5".to_string(),
            agent: None,
            finish_reason: Some("stop".to_string()),
            tokens: TokenUsage {
                input: 10,
                output: 20,
                cache_read: 0,
                cache_write: 0,
            },
            created_at: Some(updated_at),
            completed_at: Some(updated_at),
            stored_cost_usd: None,
            source: DataSourceKind::Json,
        }];

        let sqlite_like = AppData {
            events: events.clone(),
            messages: messages.clone(),
            session_records: vec![SessionRecord {
                session_id: "ses_1".to_string(),
                created_at,
                updated_at,
            }],
            import_stats: ImportStats::default(),
            sessions: Vec::new(),
            source: DataSourceKind::Sqlite,
        };
        let json_like = AppData {
            events,
            messages,
            session_records: Vec::new(),
            import_stats: ImportStats::default(),
            sessions: Vec::new(),
            source: DataSourceKind::Json,
        };
        let pricing = PricingCatalog {
            models: BTreeMap::new(),
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: PricingAvailability::Empty,
            load_notice: None,
        };

        let sqlite_snapshot = build_snapshot(
            &sqlite_like,
            &pricing,
            TimeRange::All,
            ZeroCostBehavior::KeepZero,
        );
        let json_snapshot = build_snapshot(
            &json_like,
            &pricing,
            TimeRange::All,
            ZeroCostBehavior::KeepZero,
        );

        assert_eq!(sqlite_snapshot.overview.sessions, 1);
        assert_eq!(sqlite_snapshot.overview.messages, 2);
        assert_eq!(sqlite_snapshot.overview.prompts, 1);
        assert_eq!(
            sqlite_snapshot.overview.sessions,
            json_snapshot.overview.sessions
        );
        assert_eq!(
            sqlite_snapshot.overview.messages,
            json_snapshot.overview.messages
        );
        assert_eq!(
            sqlite_snapshot.overview.prompts,
            json_snapshot.overview.prompts
        );
        assert_eq!(
            sqlite_snapshot.overview.total_tokens,
            json_snapshot.overview.total_tokens
        );
        assert_eq!(
            sqlite_snapshot.overview.models_used,
            json_snapshot.overview.models_used
        );
    }

    #[test]
    fn overview_prefers_session_records_for_session_count() {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let updated_at = Local
            .with_ymd_and_hms(2026, 3, 12, 10, 0, 0)
            .single()
            .unwrap();

        let data = AppData {
            events: Vec::new(),
            messages: Vec::new(),
            session_records: vec![SessionRecord {
                session_id: "ses_1".to_string(),
                created_at,
                updated_at,
            }],
            import_stats: ImportStats::default(),
            sessions: Vec::new(),
            source: DataSourceKind::Sqlite,
        };
        let pricing = PricingCatalog {
            models: BTreeMap::new(),
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: PricingAvailability::Empty,
            load_notice: None,
        };

        let snapshot = build_snapshot(&data, &pricing, TimeRange::All, ZeroCostBehavior::KeepZero);

        assert_eq!(snapshot.overview.sessions, 1);
    }

    #[test]
    fn overview_cost_estimates_when_stored_zero_looks_like_placeholder() {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let mut models = BTreeMap::new();
        models.insert(
            "openai/gpt-5".to_string(),
            crate::cache::models_cache::ModelPricing {
                input: rust_decimal::Decimal::new(100, 0),
                output: rust_decimal::Decimal::new(100, 0),
                cache_write: rust_decimal::Decimal::ZERO,
                cache_read: rust_decimal::Decimal::ZERO,
                context_window: 0,
                session_quota: rust_decimal::Decimal::ZERO,
            },
        );
        let data = AppData {
            events: vec![UsageEvent {
                session_id: "ses_1".to_string(),
                parent_session_id: None,
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("openai".to_string()),
                model_id: "gpt-5".to_string(),
                agent: None,
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 1_000_000,
                    output: 1_000_000,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: Some(rust_decimal::Decimal::ZERO),
                source: DataSourceKind::Json,
            }],
            messages: Vec::new(),
            session_records: Vec::new(),
            import_stats: ImportStats::default(),
            sessions: Vec::new(),
            source: DataSourceKind::Json,
        };
        let pricing = PricingCatalog {
            models,
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: PricingAvailability::Cached,
            load_notice: None,
        };

        let snapshot = build_snapshot(
            &data,
            &pricing,
            TimeRange::All,
            ZeroCostBehavior::EstimateWhenZero,
        );

        assert_eq!(
            snapshot.overview.total_cost.known,
            rust_decimal::Decimal::new(200, 0)
        );
        assert!(snapshot.overview.total_cost.has_known);
        assert!(!snapshot.overview.total_cost.missing);
    }

    #[test]
    fn overview_cost_can_keep_zero_when_requested() {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let mut models = BTreeMap::new();
        models.insert(
            "openai/gpt-5".to_string(),
            crate::cache::models_cache::ModelPricing {
                input: rust_decimal::Decimal::new(100, 0),
                output: rust_decimal::Decimal::new(100, 0),
                cache_write: rust_decimal::Decimal::ZERO,
                cache_read: rust_decimal::Decimal::ZERO,
                context_window: 0,
                session_quota: rust_decimal::Decimal::ZERO,
            },
        );
        let data = AppData {
            events: vec![UsageEvent {
                session_id: "ses_1".to_string(),
                parent_session_id: None,
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("openai".to_string()),
                model_id: "gpt-5".to_string(),
                agent: None,
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 1_000_000,
                    output: 1_000_000,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: Some(rust_decimal::Decimal::ZERO),
                source: DataSourceKind::Json,
            }],
            messages: Vec::new(),
            session_records: Vec::new(),
            import_stats: ImportStats::default(),
            sessions: Vec::new(),
            source: DataSourceKind::Json,
        };
        let pricing = PricingCatalog {
            models,
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: PricingAvailability::Cached,
            load_notice: None,
        };

        let snapshot = build_snapshot(&data, &pricing, TimeRange::All, ZeroCostBehavior::KeepZero);

        assert_eq!(
            snapshot.overview.total_cost.known,
            rust_decimal::Decimal::ZERO
        );
        assert!(snapshot.overview.total_cost.has_known);
        assert!(!snapshot.overview.total_cost.missing);
    }
}
