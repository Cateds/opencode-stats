use rust_decimal::Decimal;

use crate::utils::pricing::PriceSummary;

pub fn format_tokens(value: u64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.2}K", value as f64 / 1_000.0),
        1_000_000..=999_999_999 => format!("{:.2}M", value as f64 / 1_000_000.0),
        _ => format!("{:.2}B", value as f64 / 1_000_000_000.0),
    }
}

pub fn format_usd_precise(value: Decimal) -> String {
    if value >= Decimal::ONE {
        format!("${:.2}", value.round_dp(2))
    } else if value == Decimal::ZERO {
        "$0.00".to_string()
    } else {
        format!("${:.4}", value.round_dp(4))
    }
}

pub fn format_price_summary(value: &PriceSummary) -> String {
    if !value.has_known {
        return if value.missing {
            "--".to_string()
        } else {
            format_usd_precise(Decimal::ZERO)
        };
    }

    let amount = format_usd_precise(value.known);
    if value.missing {
        format!("{amount} + ?")
    } else {
        amount
    }
}

pub fn tokens_comparison_text(total_tokens: u64) -> String {
    let tokens = total_tokens as f64;

    // Base reading speed: ~225 words/min -> ~300 tokens/min
    const MIN_READING: f64 = 300.0;
    const HOUR_READING: f64 = MIN_READING * 60.0; // 18,000 tokens
    const DAY_COZY_READING: f64 = HOUR_READING * 4.0; // 72,000 tokens (4 hours/day)

    // Writing/Speaking milestones
    const LETTER: f64 = 600.0; // ~450 words per letter
    const NOTEBOOK: f64 = 30_000.0; // ~22,500 words per notebook
    const NOVEL: f64 = 150_000.0; // ~112,500 words per thick novel
    const BOOKSHELF: f64 = NOVEL * 40.0; // 6,000,000 tokens (40 novels)
    const TOWN_LIBRARY: f64 = NOVEL * 50_000.0; // 7,500,000,000 tokens (50k books)
    const NATIONAL_LIBRARY: f64 = NOVEL * 20_000_000.0; // 3,000,000,000,000 tokens (20M books)

    const YEAR_JOURNALING: f64 = 250_000.0; // ~500 words/day * 365
    const YEAR_SPEAKING: f64 = 7_500_000.0; // ~16,000 words/day * 365
    const LIFETIME_SPEAKING: f64 = YEAR_SPEAKING * 80.0; // 600,000,000 tokens (80 years)
    const MILLENNIUM_SPEAKING: f64 = YEAR_SPEAKING * 1000.0; // 7,500,000,000 tokens
    const EPOCH_CIVILIZATION: f64 = YEAR_SPEAKING * 100_000.0; // 750,000,000,000 tokens

    match total_tokens {
        0 => "No activity yet. Start a conversation!".to_string(),
        1..10_000 => format!(
            "About {:.1} handwritten letters, or {:.0} mins of reading.",
            tokens / LETTER,
            tokens / MIN_READING
        ),
        10_000..250_000 => format!(
            "Roughly {:.1} filled notebooks, or {:.1} hours of reading.",
            tokens / NOTEBOOK,
            tokens / HOUR_READING
        ),
        250_000..5_000_000 => format!(
            "Around {:.1} thick novels, or {:.1} days of cozy reading.",
            tokens / NOVEL,
            tokens / DAY_COZY_READING
        ),
        5_000_000..50_000_000 => format!(
            "Like {:.1} packed bookshelves, or {:.1} years of journaling.",
            tokens / BOOKSHELF,
            tokens / YEAR_JOURNALING
        ),
        50_000_000..5_000_000_000 => format!(
            "About {:.1} years of daily speaking, or {:.2} lifetimes of words.",
            tokens / YEAR_SPEAKING,
            tokens / LIFETIME_SPEAKING
        ),
        5_000_000_000..500_000_000_000 => format!(
            "Like {:.2} town libraries, or {:.1} millennia of human speech.",
            tokens / TOWN_LIBRARY,
            tokens / MILLENNIUM_SPEAKING
        ),
        _ => format!(
            "Roughly {:.2} national libraries, or {:.2} epochs of civilization.",
            tokens / NATIONAL_LIBRARY,
            tokens / EPOCH_CIVILIZATION
        ),
    }
}

pub fn percentage(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}
