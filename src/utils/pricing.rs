use rust_decimal::Decimal;

use crate::cache::models_cache::{PricingCatalog, price_tokens};
use crate::db::models::UsageEvent;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ZeroCostBehavior {
    #[default]
    EstimateWhenZero,
    KeepZero,
}

#[derive(Clone, Debug, Default)]
pub struct PriceSummary {
    pub known: Decimal,
    pub has_known: bool,
    pub missing: bool,
}

impl PriceSummary {
    pub fn add_known(&mut self, amount: Decimal) {
        self.known += amount;
        self.has_known = true;
    }

    pub fn add_missing(&mut self) {
        self.missing = true;
    }

    pub fn merge(&mut self, other: &Self) {
        self.known += other.known;
        self.has_known |= other.has_known;
        self.missing |= other.missing;
    }
}

pub fn update_price_summary(
    summary: &mut PriceSummary,
    pricing: &PricingCatalog,
    event: &UsageEvent,
    zero_cost_behavior: ZeroCostBehavior,
) {
    if let Some(cost) = resolved_stored_cost(event, zero_cost_behavior) {
        summary.add_known(cost);
        return;
    }

    if let Some(model_pricing) = pricing.lookup_for_event(event) {
        summary.add_known(price_tokens(&event.tokens, model_pricing));
    } else {
        summary.add_missing();
    }
}

fn resolved_stored_cost(
    event: &UsageEvent,
    zero_cost_behavior: ZeroCostBehavior,
) -> Option<Decimal> {
    let cost = event.stored_cost_usd?;
    match zero_cost_behavior {
        ZeroCostBehavior::KeepZero => Some(cost),
        ZeroCostBehavior::EstimateWhenZero => {
            if cost.is_zero() && event.tokens.total() > 0 {
                None
            } else {
                Some(cost)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PriceSummary, ZeroCostBehavior, update_price_summary};
    use crate::cache::models_cache::{ModelPricing, PricingAvailability, PricingCatalog};
    use crate::db::models::{DataSourceKind, TokenUsage, UsageEvent};
    use chrono::{Local, TimeZone};
    use rust_decimal::Decimal;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn pricing_catalog() -> PricingCatalog {
        let mut models = BTreeMap::new();
        models.insert(
            "openai/gpt-5".to_string(),
            ModelPricing {
                input: Decimal::new(100, 0),
                output: Decimal::new(200, 0),
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
    fn estimates_cost_when_zero_is_placeholder() {
        let pricing = pricing_catalog();
        let event = usage_event(
            Some(Decimal::ZERO),
            TokenUsage {
                input: 1_000_000,
                output: 1_000_000,
                cache_read: 0,
                cache_write: 0,
            },
        );
        let mut summary = PriceSummary::default();

        update_price_summary(
            &mut summary,
            &pricing,
            &event,
            ZeroCostBehavior::EstimateWhenZero,
        );

        assert_eq!(summary.known, Decimal::new(300, 0));
        assert!(summary.has_known);
        assert!(!summary.missing);
    }

    #[test]
    fn keeps_zero_when_flag_requests_original_behavior() {
        let pricing = pricing_catalog();
        let event = usage_event(
            Some(Decimal::ZERO),
            TokenUsage {
                input: 1_000_000,
                output: 1_000_000,
                cache_read: 0,
                cache_write: 0,
            },
        );
        let mut summary = PriceSummary::default();

        update_price_summary(&mut summary, &pricing, &event, ZeroCostBehavior::KeepZero);

        assert_eq!(summary.known, Decimal::ZERO);
        assert!(summary.has_known);
        assert!(!summary.missing);
    }

    #[test]
    fn keeps_true_zero_cost_for_zero_token_events() {
        let pricing = pricing_catalog();
        let event = usage_event(Some(Decimal::ZERO), TokenUsage::default());
        let mut summary = PriceSummary::default();

        update_price_summary(
            &mut summary,
            &pricing,
            &event,
            ZeroCostBehavior::EstimateWhenZero,
        );

        assert_eq!(summary.known, Decimal::ZERO);
        assert!(summary.has_known);
        assert!(!summary.missing);
    }
}
