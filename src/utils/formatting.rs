use rust_decimal::Decimal;

pub fn format_tokens(value: u64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.1}K", value as f64 / 1_000.0),
        1_000_000..=999_999_999 => format!("{:.1}M", value as f64 / 1_000_000.0),
        _ => format!("{:.1}B", value as f64 / 1_000_000_000.0),
    }
}

pub fn format_usd_precise(value: Decimal) -> String {
    if value >= Decimal::ONE {
        format!("${:.2}", value.round_dp(2))
    } else {
        format!("${:.4}", value.round_dp(4))
    }
}

pub fn tokens_comparison_text(total_tokens: u64) -> String {
    if total_tokens == 0 {
        return "No activity yet. Once OpenCode starts spending tokens, this area turns into the fun part.".to_string();
    }

    let pages = total_tokens as f64 / 750.0;
    let novels = pages / 300.0;
    let hours_of_reading = pages / 40.0;

    if novels >= 1.0 {
        format!(
            "That is roughly {:.1} novel(s) of text, or about {:.1} hours of nonstop reading.",
            novels, hours_of_reading
        )
    } else {
        format!(
            "That is about {:.0} printed pages of text, or roughly {:.1} hours of reading.",
            pages, hours_of_reading
        )
    }
}

pub fn percentage(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}
