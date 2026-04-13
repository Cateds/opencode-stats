use chrono::NaiveDate;

use crate::cache::models_cache::PricingCatalog;
use crate::db::models::{TokenUsage, UsageEvent};
use crate::utils::pricing::{PriceSummary, ZeroCostBehavior, update_price_summary};

#[derive(Clone, Debug)]
pub struct DailyUsage {
    pub date: NaiveDate,
    pub tokens: TokenUsage,
    pub interactions: usize,
    pub cost: PriceSummary,
}

pub fn aggregate_daily(
    events: &[UsageEvent],
    pricing: &PricingCatalog,
    today: NaiveDate,
    zero_cost_behavior: ZeroCostBehavior,
) -> Vec<DailyUsage> {
    let mut grouped = std::collections::BTreeMap::<NaiveDate, DailyUsage>::new();
    for event in events {
        let Some(date) = event.activity_date() else {
            continue;
        };
        if date > today {
            continue;
        }
        let entry = grouped.entry(date).or_insert_with(|| DailyUsage {
            date,
            tokens: TokenUsage::default(),
            interactions: 0,
            cost: PriceSummary::default(),
        });
        entry.tokens.add_assign(&event.tokens);
        entry.interactions += 1;
        update_price_summary(&mut entry.cost, pricing, event, zero_cost_behavior);
    }

    grouped.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use chrono::{Local, NaiveDate, TimeZone};
    use rust_decimal::Decimal;

    use super::aggregate_daily;
    use crate::cache::models_cache::{ModelPricing, PricingAvailability, PricingCatalog};
    use crate::db::models::{DataSourceKind, TokenUsage, UsageEvent};
    use crate::utils::pricing::ZeroCostBehavior;

    fn pricing_catalog() -> PricingCatalog {
        let mut models = BTreeMap::new();
        models.insert(
            "openai/gpt-5".to_string(),
            ModelPricing {
                input: Decimal::new(100, 0),
                output: Decimal::new(100, 0),
                cache_write: Decimal::ZERO,
                cache_read: Decimal::ZERO,
                context_window: 0,
                session_quota: Decimal::ZERO,
            },
        );

        PricingCatalog {
            models,
            cache_path: PathBuf::from("/tmp/models.json"),
            refresh_needed: false,
            availability: PricingAvailability::Cached,
            load_notice: None,
        }
    }

    fn usage_event(stored_cost_usd: Option<Decimal>, tokens: TokenUsage) -> UsageEvent {
        let created_at = Local
            .with_ymd_and_hms(2026, 3, 12, 9, 30, 0)
            .single()
            .unwrap();

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
            agent: None,
            finish_reason: Some("stop".to_string()),
            tokens,
            created_at: Some(created_at),
            completed_at: Some(created_at),
            stored_cost_usd,
            source: DataSourceKind::Json,
        }
    }

    #[test]
    fn daily_cost_estimates_when_zero_is_ignored() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let daily = aggregate_daily(
            &[usage_event(
                Some(Decimal::ZERO),
                TokenUsage {
                    input: 1_000_000,
                    output: 1_000_000,
                    cache_read: 0,
                    cache_write: 0,
                },
            )],
            &pricing_catalog(),
            day,
            ZeroCostBehavior::EstimateWhenZero,
        );

        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].cost.known, Decimal::new(200, 0));
        assert!(daily[0].cost.has_known);
        assert!(!daily[0].cost.missing);
    }

    #[test]
    fn daily_cost_keeps_zero_when_requested() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let daily = aggregate_daily(
            &[usage_event(
                Some(Decimal::ZERO),
                TokenUsage {
                    input: 1_000_000,
                    output: 1_000_000,
                    cache_read: 0,
                    cache_write: 0,
                },
            )],
            &pricing_catalog(),
            day,
            ZeroCostBehavior::KeepZero,
        );

        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].cost.known, Decimal::ZERO);
        assert!(daily[0].cost.has_known);
        assert!(!daily[0].cost.missing);
    }

    #[test]
    fn daily_cost_keeps_true_zero_for_zero_token_events() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let daily = aggregate_daily(
            &[usage_event(Some(Decimal::ZERO), TokenUsage::default())],
            &pricing_catalog(),
            day,
            ZeroCostBehavior::EstimateWhenZero,
        );

        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].cost.known, Decimal::ZERO);
        assert!(daily[0].cost.has_known);
        assert!(!daily[0].cost.missing);
    }
}
