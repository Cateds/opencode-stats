use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;

use crate::cache::models_cache::PricingCatalog;
use crate::db::models::{MessageRecord, TokenUsage, UsageEvent};
use crate::utils::formatting::percentage;
use crate::utils::pricing::{PriceSummary, ZeroCostBehavior, update_price_summary};
use crate::utils::time::TimeRange;

#[derive(Clone, Debug)]
pub struct ModelUsageRow {
    pub model_id: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub messages: usize,
    pub prompts: usize,
    pub sessions: usize,
    pub active_days: usize,
    pub cost: PriceSummary,
    pub percentage: f64,
    pub p50_output_tokens_per_second: f64,
}

#[derive(Clone, Debug)]
pub struct ProviderUsageRow {
    pub provider_id: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub messages: usize,
    pub prompts: usize,
    pub sessions: usize,
    pub active_days: usize,
    pub cost: PriceSummary,
    pub percentage: f64,
    pub p50_output_tokens_per_second: f64,
}

#[derive(Clone, Debug)]
pub struct ModelChartSeries {
    pub model_id: String,
    pub palette_index: usize,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub struct ModelChartData {
    pub x_bounds: [f64; 2],
    pub y_bounds: [f64; 2],
    pub x_labels: Vec<String>,
    pub y_labels: Vec<String>,
    pub series: Vec<ModelChartSeries>,
}

pub fn build_model_chart(
    events: &[UsageEvent],
    messages: &[MessageRecord],
    pricing: &PricingCatalog,
    _range: TimeRange,
    today: NaiveDate,
    zero_cost_behavior: ZeroCostBehavior,
) -> (Vec<ModelUsageRow>, ModelChartData) {
    let mut model_rows = BTreeMap::<String, UsageAccumulator>::new();

    for message in messages {
        let Some(model) = message.model_id.clone() else {
            continue;
        };
        let entry = model_rows.entry(model).or_default();
        if message.role.as_deref() == Some("assistant") {
            entry.messages += 1;
        }
        if message.role.as_deref() == Some("user") {
            entry.prompts += 1;
        }
    }

    for event in events {
        let model = event.model_id.clone();
        let entry = model_rows.entry(model).or_default();
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
        }
    }

    let overall_tokens = model_rows
        .values()
        .map(|row| row.tokens.total())
        .fold(0u64, |total, value| total.saturating_add(value));
    let mut rows = model_rows
        .into_iter()
        .map(|(model_id, row)| ModelUsageRow {
            model_id,
            total_tokens: row.tokens.total(),
            input_tokens: row.tokens.input,
            output_tokens: row.tokens.output,
            percentage: percentage(row.tokens.total(), overall_tokens),
            messages: row.messages,
            prompts: row.prompts,
            sessions: row.sessions.len(),
            active_days: row.active_days.len(),
            cost: row.cost,
            p50_output_tokens_per_second: median(&row.output_rates),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| right.total_tokens.cmp(&left.total_tokens));

    let top_models = rows
        .iter()
        .map(|row| row.model_id.clone())
        .collect::<Vec<_>>();
    let chart = build_chart_for_models(events, &top_models, today, |event| event.model_id.clone());
    (rows, chart)
}

pub fn build_provider_chart(
    events: &[UsageEvent],
    messages: &[MessageRecord],
    pricing: &PricingCatalog,
    _range: TimeRange,
    today: NaiveDate,
    zero_cost_behavior: ZeroCostBehavior,
) -> (Vec<ProviderUsageRow>, ModelChartData) {
    let mut provider_rows = BTreeMap::<String, UsageAccumulator>::new();

    for message in messages {
        let provider = message
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let entry = provider_rows.entry(provider).or_default();
        if message.role.as_deref() == Some("assistant") {
            entry.messages += 1;
        }
        if message.role.as_deref() == Some("user") {
            entry.prompts += 1;
        }
    }

    for event in events {
        let provider = event
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let entry = provider_rows.entry(provider).or_default();
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
        }
    }

    let overall_tokens = provider_rows
        .values()
        .map(|row| row.tokens.total())
        .fold(0u64, |total, value| total.saturating_add(value));
    let mut rows = provider_rows
        .into_iter()
        .map(|(provider_id, row)| ProviderUsageRow {
            provider_id,
            total_tokens: row.tokens.total(),
            input_tokens: row.tokens.input,
            output_tokens: row.tokens.output,
            percentage: percentage(row.tokens.total(), overall_tokens),
            messages: row.messages,
            prompts: row.prompts,
            sessions: row.sessions.len(),
            active_days: row.active_days.len(),
            cost: row.cost,
            p50_output_tokens_per_second: median(&row.output_rates),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| right.total_tokens.cmp(&left.total_tokens));

    let providers = rows
        .iter()
        .map(|row| row.provider_id.clone())
        .collect::<Vec<_>>();
    let chart = build_chart_for_models(events, &providers, today, |event| {
        event
            .provider_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    });
    (rows, chart)
}

pub fn chart_with_focus(chart: &ModelChartData, focused_model_id: Option<&str>) -> ModelChartData {
    let mut series = chart.series.clone();

    if let Some(model_id) = focused_model_id
        && let Some(index) = series.iter().position(|series| series.model_id == model_id)
    {
        let focused = series.remove(index);
        series.push(focused);
    }

    ModelChartData {
        x_bounds: chart.x_bounds,
        y_bounds: chart.y_bounds,
        x_labels: chart.x_labels.clone(),
        y_labels: chart.y_labels.clone(),
        series,
    }
}

fn build_chart_for_models<F>(
    events: &[UsageEvent],
    top_models: &[String],
    today: NaiveDate,
    key_fn: F,
) -> ModelChartData
where
    F: Fn(&UsageEvent) -> String,
{
    let mut daily_values = BTreeMap::<String, BTreeMap<NaiveDate, u64>>::new();
    let mut min_date = today;
    let mut max_date = today;
    let mut has_dates = false;

    for event in events {
        let key = key_fn(event);
        if !top_models.contains(&key) {
            continue;
        }
        let Some(date) = event.activity_date() else {
            continue;
        };
        has_dates = true;
        if date < min_date {
            min_date = date;
        }
        if date > max_date {
            max_date = date;
        }
        let total = daily_values
            .entry(key)
            .or_default()
            .entry(date)
            .or_default();
        *total = total.saturating_add(event.tokens.total());
    }

    if !has_dates {
        return ModelChartData {
            x_bounds: [0.0, 1.0],
            y_bounds: [0.0, 1.0],
            x_labels: vec!["Start".to_string(), "End".to_string()],
            y_labels: vec!["0".to_string(), "1".to_string()],
            series: Vec::new(),
        };
    }

    let mut days = Vec::new();
    let mut cursor = min_date;
    while cursor <= max_date {
        days.push(cursor);
        cursor += chrono::Duration::days(1);
    }

    let mut y_max = 1f64;
    let mut series = Vec::new();
    for (palette_index, model_id) in top_models.iter().enumerate() {
        let points = days
            .iter()
            .enumerate()
            .map(|(index, day)| {
                let value = daily_values
                    .get(model_id)
                    .and_then(|map| map.get(day))
                    .copied()
                    .unwrap_or_default();
                y_max = y_max.max(value as f64);
                (index as f64, value as f64)
            })
            .collect::<Vec<_>>();
        series.push(ModelChartSeries {
            model_id: model_id.clone(),
            palette_index,
            points,
        });
    }

    let last_index = (days.len().saturating_sub(1)) as f64;
    let x_labels = build_x_labels(&days);
    let (y_ticks, y_bounds) = nice_integer_ticks(y_max.max(1.0), 4);
    let y_labels = y_ticks
        .iter()
        .map(|tick| format_tick_label(*tick))
        .collect::<Vec<_>>();

    ModelChartData {
        x_bounds: [0.0, last_index.max(1.0)],
        y_bounds,
        x_labels,
        y_labels,
        series,
    }
}

fn build_x_labels(days: &[NaiveDate]) -> Vec<String> {
    match days {
        [] => vec!["Start".to_string(), "Mid".to_string(), "End".to_string()],
        [day] => {
            let label = day.format("%b %d").to_string();
            vec![label.clone(), label.clone(), label]
        }
        _ => {
            let middle_index = days.len() / 2;
            vec![
                days[0].format("%b %d").to_string(),
                days[middle_index].format("%b %d").to_string(),
                days[days.len() - 1].format("%b %d").to_string(),
            ]
        }
    }
}

fn nice_integer_ticks(max_value: f64, desired_steps: usize) -> (Vec<f64>, [f64; 2]) {
    let desired_steps = desired_steps.max(2);
    let step = nice_step((max_value / desired_steps as f64).ceil().max(1.0));
    let upper_bound = (max_value / step).ceil() * step;
    let tick_count = (upper_bound / step).round() as usize;
    let ticks = (0..=tick_count)
        .map(|index| index as f64 * step)
        .collect::<Vec<_>>();

    (ticks, [0.0, upper_bound.max(1.0)])
}

fn nice_step(rough_step: f64) -> f64 {
    if rough_step <= 1.0 {
        return 1.0;
    }

    let magnitude = 10_f64.powf(rough_step.log10().floor());
    let normalized = rough_step / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };

    (nice * magnitude).round()
}

fn format_tick_label(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.0}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.0}K", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}

#[derive(Default)]
struct UsageAccumulator {
    tokens: TokenUsage,
    messages: usize,
    prompts: usize,
    sessions: BTreeSet<String>,
    active_days: BTreeSet<NaiveDate>,
    cost: PriceSummary,
    daily_tokens: BTreeMap<NaiveDate, u64>,
    output_rates: Vec<f64>,
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut values = values.to_vec();
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::build_chart_for_models;
    use crate::db::models::{DataSourceKind, TokenUsage, UsageEvent};
    use chrono::{Local, NaiveDate, TimeZone};

    #[test]
    fn single_day_chart_builds_labels_without_unwrap_assumptions() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();
        let chart = build_chart_for_models(
            &[UsageEvent {
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
                finish_reason: None,
                tokens: TokenUsage {
                    input: 1,
                    output: 2,
                    cache_read: 0,
                    cache_write: 0,
                },
                created_at: Some(created_at),
                completed_at: Some(created_at),
                stored_cost_usd: None,
                source: DataSourceKind::Json,
            }],
            &["gpt-5".to_string()],
            day,
            |event| event.model_id.clone(),
        );

        assert_eq!(chart.x_labels.len(), 3);
        assert_eq!(chart.x_labels[0], "Mar 12");
        assert_eq!(chart.x_labels[2], "Mar 12");
    }
}
