use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use crate::cache::models_cache::PricingCatalog;
use crate::db::models::{TokenUsage, UsageEvent};
use crate::utils::formatting::percentage;
use crate::utils::pricing::{PriceSummary, ZeroCostBehavior, update_price_summary};
use crate::utils::time::TimeRange;

use super::model_stats::UsageAccumulator;

#[derive(Clone, Debug)]
pub struct AgentModelBreakdown {
    pub model_id: String,
    pub tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub cost: PriceSummary,
    pub sessions: usize,
    pub active_days: usize,
    pub p50_output_tokens_per_second: f64,
}

#[derive(Clone, Debug)]
pub struct AgentUsageRow {
    pub agent_id: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub sessions: usize,
    pub active_days: usize,
    pub cost: PriceSummary,
    pub percentage: f64,
    pub p50_output_tokens_per_second: f64,
    pub model_breakdown: Vec<AgentModelBreakdown>,
}

use super::model_stats::{ModelChartData, build_chart_for_models, median};

pub fn build_agent_chart(
    events: &[&UsageEvent],
    pricing: &PricingCatalog,
    range: TimeRange,
    today: NaiveDate,
    zero_cost_behavior: ZeroCostBehavior,
) -> (Vec<AgentUsageRow>, ModelChartData, Vec<(String, ModelChartData)>) {
    let mut agent_rows = BTreeMap::<String, UsageAccumulator>::new();
    let mut agent_model_tokens = BTreeMap::<String, BTreeMap<String, TokenUsage>>::new();
    let mut agent_model_cost = BTreeMap::<String, BTreeMap<String, PriceSummary>>::new();
    let mut agent_model_sessions = BTreeMap::<String, BTreeMap<String, BTreeSet<String>>>::new();
    let mut agent_model_days = BTreeMap::<String, BTreeMap<String, BTreeSet<NaiveDate>>>::new();
    let mut agent_model_rates = BTreeMap::<String, BTreeMap<String, Vec<f64>>>::new();

    for event in events {
        let agent = event
            .agent
            .clone()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let model = event.model_id.clone();

        let entry = agent_rows.entry(agent.clone()).or_default();
        entry.tokens.add_assign(&event.tokens);
        entry.sessions.insert(event.session_id.clone());
        update_price_summary(&mut entry.cost, pricing, event, zero_cost_behavior);
        if let Some(date) = event.activity_date() {
            entry.active_days.insert(date);
            let total = entry.daily_tokens.entry(date).or_default();
            *total = total.saturating_add(event.tokens.total());
        }
        if event.is_rate_eligible()
            && let Some(duration_ms) = event.duration_ms()
        {
            let rate = event.tokens.output as f64 / (duration_ms as f64 / 1_000.0);
            entry.output_rates.push(rate);
            agent_model_rates
                .entry(agent.clone())
                .or_default()
                .entry(model.clone())
                .or_default()
                .push(rate);
        }

        let model_tokens = agent_model_tokens
            .entry(agent.clone())
            .or_default()
            .entry(model.clone())
            .or_default();
        model_tokens.add_assign(&event.tokens);

        let model_cost = agent_model_cost
            .entry(agent.clone())
            .or_default()
            .entry(model.clone())
            .or_default();
        update_price_summary(model_cost, pricing, event, zero_cost_behavior);

        agent_model_sessions
            .entry(agent.clone())
            .or_default()
            .entry(model.clone())
            .or_default()
            .insert(event.session_id.clone());

        if let Some(date) = event.activity_date() {
            agent_model_days
                .entry(agent.clone())
                .or_default()
                .entry(model.clone())
                .or_default()
                .insert(date);
        }
    }

    let overall_tokens = agent_rows
        .values()
        .map(|row| row.tokens.total())
        .fold(0u64, |total, value| total.saturating_add(value));
    let mut rows = agent_rows
        .into_iter()
        .map(|(agent_id, row)| {
            let model_breakdown = agent_model_tokens
                .get(&agent_id)
                .map(|models| {
                    let mut breakdown: Vec<AgentModelBreakdown> = models
                        .iter()
                        .map(|(model_id, tokens)| AgentModelBreakdown {
                            model_id: model_id.clone(),
                            tokens: tokens.total(),
                            input_tokens: tokens.input,
                            output_tokens: tokens.output,
                            cache_tokens: tokens
                                .cache_read
                                .saturating_add(tokens.cache_write),
                            cost: agent_model_cost
                                .get(&agent_id)
                                .and_then(|costs| costs.get(model_id).cloned())
                                .unwrap_or_default(),
                            sessions: agent_model_sessions
                                .get(&agent_id)
                                .and_then(|sessions| sessions.get(model_id))
                                .map(|s| s.len())
                                .unwrap_or(0),
                            active_days: agent_model_days
                                .get(&agent_id)
                                .and_then(|days| days.get(model_id))
                                .map(|d| d.len())
                                .unwrap_or(0),
                            p50_output_tokens_per_second: agent_model_rates
                                .get(&agent_id)
                                .and_then(|rates| rates.get(model_id))
                                .map(|r| median(r))
                                .unwrap_or(0.0),
                        })
                        .collect();
                    breakdown.sort_by_key(|b| std::cmp::Reverse(b.tokens));
                    breakdown
                })
                .unwrap_or_default();

            AgentUsageRow {
                agent_id,
                total_tokens: row.tokens.total(),
                input_tokens: row.tokens.input,
                output_tokens: row.tokens.output,
                cache_tokens: row.tokens.cache_read.saturating_add(row.tokens.cache_write),
                percentage: percentage(row.tokens.total(), overall_tokens),
                sessions: row.sessions.len(),
                active_days: row.active_days.len(),
                cost: row.cost,
                p50_output_tokens_per_second: median(&row.output_rates),
                model_breakdown,
            }
        })
        .collect::<Vec<_>>();

    rows.sort_by_key(|right| std::cmp::Reverse(right.total_tokens));

    let top_agents = rows
        .iter()
        .map(|row| row.agent_id.clone())
        .collect::<Vec<_>>();
    let chart = build_chart_for_models(events, &top_agents, range, today, |event| {
        event
            .agent
            .clone()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    });

    let mut agent_model_charts = Vec::new();
    for (agent_id, models) in &agent_model_tokens {
        let model_ids: Vec<String> = models.keys().cloned().collect();
        let agent_events: Vec<&UsageEvent> = events
            .iter()
            .filter(|event| {
                event
                    .agent
                    .as_deref()
                    .filter(|a| !a.is_empty())
                    .unwrap_or("unknown")
                    == agent_id.as_str()
            })
            .copied()
            .collect();
        let agent_chart = build_chart_for_models(
            &agent_events,
            &model_ids,
            range,
            today,
            |event| event.model_id.clone(),
        );
        agent_model_charts.push((agent_id.clone(), agent_chart));
    }

    (rows, chart, agent_model_charts)
}

#[cfg(test)]
mod tests {
    use super::build_agent_chart;
    use crate::db::models::{DataSourceKind, TokenUsage, UsageEvent};
    use crate::utils::time::TimeRange;
    use chrono::{Local, TimeZone};

    #[test]
    fn agents_group_events_by_agent_field() {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let day = created_at.date_naive();
        let events = vec![
            UsageEvent {
                session_id: "ses_1".to_string(),
                parent_session_id: None,
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("openai".to_string()),
                model_id: "gpt-5".to_string(),
                agent: Some("build".to_string()),
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 100,
                    output: 200,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: None,
                source: DataSourceKind::Json,
            },
            UsageEvent {
                session_id: "ses_2".to_string(),
                parent_session_id: Some("ses_1".to_string()),
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("anthropic".to_string()),
                model_id: "claude-sonnet".to_string(),
                agent: Some("explore".to_string()),
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 50,
                    output: 100,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: None,
                source: DataSourceKind::Json,
            },
            UsageEvent {
                session_id: "ses_3".to_string(),
                parent_session_id: None,
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("openai".to_string()),
                model_id: "gpt-5.5".to_string(),
                agent: Some("build".to_string()),
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 300,
                    output: 400,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: None,
                source: DataSourceKind::Json,
            },
            UsageEvent {
                session_id: "ses_1".to_string(),
                parent_session_id: None,
                session_title: None,
                session_started_at: Some(created_at),
                session_archived_at: None,
                project_name: None,
                project_path: None,
                provider_id: Some("unknown".to_string()),
                model_id: "unknown-model".to_string(),
                agent: None,
                finish_reason: Some("stop".to_string()),
                tokens: TokenUsage {
                    input: 10,
                    output: 20,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: None,
                source: DataSourceKind::Json,
            },
        ];

        let pricing = crate::cache::models_cache::PricingCatalog {
            models: std::collections::BTreeMap::new(),
            cache_path: std::path::PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: crate::cache::models_cache::PricingAvailability::Empty,
            load_notice: None,
        };
        let (rows, _chart, _agent_model_charts) = build_agent_chart(
            &events.iter().collect::<Vec<_>>(),
            &pricing,
            TimeRange::All,
            day,
            crate::utils::pricing::ZeroCostBehavior::KeepZero,
        );

        assert_eq!(rows.len(), 3);

        assert_eq!(rows[0].agent_id, "build");
        assert_eq!(rows[0].total_tokens, 1000);
        assert_eq!(rows[0].sessions, 2);
        assert_eq!(rows[0].input_tokens, 400);
        assert_eq!(rows[0].output_tokens, 600);
        assert_eq!(rows[0].cache_tokens, 0);
        assert_eq!(rows[0].active_days, 1);
        assert!((rows[0].p50_output_tokens_per_second - 0.0).abs() < f64::EPSILON);
        assert_eq!(rows[0].model_breakdown.len(), 2);
        assert_eq!(rows[0].model_breakdown[0].model_id, "gpt-5.5");
        assert_eq!(rows[0].model_breakdown[0].tokens, 700);
        assert_eq!(rows[0].model_breakdown[0].input_tokens, 300);
        assert_eq!(rows[0].model_breakdown[0].output_tokens, 400);
        assert_eq!(rows[0].model_breakdown[0].cache_tokens, 0);
        assert_eq!(rows[0].model_breakdown[0].sessions, 1);
        assert_eq!(rows[0].model_breakdown[0].active_days, 1);
        assert!((rows[0].model_breakdown[0].p50_output_tokens_per_second - 0.0).abs() < f64::EPSILON);
        assert_eq!(rows[0].model_breakdown[1].model_id, "gpt-5");
        assert_eq!(rows[0].model_breakdown[1].tokens, 300);
        assert_eq!(rows[0].model_breakdown[1].input_tokens, 100);
        assert_eq!(rows[0].model_breakdown[1].output_tokens, 200);
        assert_eq!(rows[0].model_breakdown[1].cache_tokens, 0);
        assert_eq!(rows[0].model_breakdown[1].sessions, 1);
        assert_eq!(rows[0].model_breakdown[1].active_days, 1);

        assert_eq!(rows[1].agent_id, "explore");
        assert_eq!(rows[1].total_tokens, 150);
        assert_eq!(rows[1].sessions, 1);
        assert_eq!(rows[1].input_tokens, 50);
        assert_eq!(rows[1].output_tokens, 100);
        assert_eq!(rows[1].cache_tokens, 0);
        assert_eq!(rows[1].active_days, 1);
        assert!(rows[1].p50_output_tokens_per_second < f64::EPSILON);
        assert_eq!(rows[1].model_breakdown.len(), 1);
        assert_eq!(rows[1].model_breakdown[0].model_id, "claude-sonnet");
        assert_eq!(rows[1].model_breakdown[0].tokens, 150);
        assert_eq!(rows[1].model_breakdown[0].input_tokens, 50);
        assert_eq!(rows[1].model_breakdown[0].output_tokens, 100);
        assert_eq!(rows[1].model_breakdown[0].cache_tokens, 0);
        assert_eq!(rows[1].model_breakdown[0].sessions, 1);
        assert_eq!(rows[1].model_breakdown[0].active_days, 1);

        assert_eq!(rows[2].agent_id, "unknown");
        assert_eq!(rows[2].total_tokens, 30);
        assert_eq!(rows[2].model_breakdown.len(), 1);
        assert_eq!(rows[2].model_breakdown[0].model_id, "unknown-model");
        assert_eq!(rows[2].model_breakdown[0].tokens, 30);
        assert_eq!(rows[2].model_breakdown[0].sessions, 1);
        assert_eq!(rows[2].input_tokens, 10);
        assert_eq!(rows[2].output_tokens, 20);
        assert_eq!(rows[2].cache_tokens, 0);
        assert_eq!(rows[2].active_days, 1);
    }
}
