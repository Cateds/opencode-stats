use crate::cache::models_cache::PricingCatalog;
use crate::db::models::{TokenUsage, UsageEvent};
use crate::utils::pricing::PriceSummary;
use chrono::NaiveDate;
use rust_decimal::Decimal;

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
        if let Some(cost) = event.stored_cost_usd
            && cost > Decimal::ZERO
        {
            entry.cost.add_known(cost);
            continue;
        }

        if pricing.has_pricing_for_event(event) {
            entry.cost.add_known(pricing.cost_for_event(event));
        } else {
            entry.cost.add_missing();
        }
    }

    grouped.into_values().collect()
}
