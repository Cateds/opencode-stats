use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::db::models::UsageEvent;

#[derive(Clone, Debug)]
pub struct HeatmapCell {
    pub intensity: u8,
    pub is_future: bool,
}

#[derive(Clone, Debug)]
pub struct HeatmapData {
    pub weeks: Vec<Vec<HeatmapCell>>,
    pub month_labels: Vec<(usize, String)>,
}

pub fn build_heatmap_data(events: &[UsageEvent], today: NaiveDate) -> HeatmapData {
    let end = today;
    let start = today - Duration::days(364);
    let grid_start = start - Duration::days(start.weekday().num_days_from_monday() as i64);
    let grid_end = end + Duration::days((6 - end.weekday().num_days_from_monday()) as i64);

    let mut totals = std::collections::BTreeMap::<NaiveDate, u64>::new();
    for event in events {
        let Some(date) = event.activity_date() else {
            continue;
        };
        if date < start || date > end {
            continue;
        }
        let total = totals.entry(date).or_default();
        *total = total.saturating_add(event.tokens.total());
    }

    let max = totals.values().copied().max().unwrap_or_default();
    let mut date = grid_start;
    let mut weeks = Vec::new();
    let mut month_labels = Vec::new();
    let mut week_index = 0usize;
    while date <= grid_end {
        let mut week = Vec::new();
        for _ in 0..7 {
            let tokens = totals.get(&date).copied().unwrap_or_default();
            let intensity = map_intensity(tokens, max);
            week.push(HeatmapCell {
                intensity,
                is_future: date > end,
            });
            if date.day() == 1 {
                let label = date.format("%b").to_string();
                if month_labels
                    .last()
                    .map(|(_, current)| current != &label)
                    .unwrap_or(true)
                {
                    month_labels.push((week_index, label));
                }
            }
            date += Duration::days(1);
        }
        weeks.push(week);
        week_index += 1;
    }

    HeatmapData {
        weeks,
        month_labels,
    }
}

fn map_intensity(tokens: u64, max: u64) -> u8 {
    if tokens == 0 || max == 0 {
        0
    } else {
        let ratio = tokens as f64 / max as f64;
        if ratio < 0.15 {
            1
        } else if ratio < 0.4 {
            2
        } else if ratio < 0.75 {
            3
        } else {
            4
        }
    }
}

pub fn weekday_labels() -> [Weekday; 7] {
    [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ]
}

#[cfg(test)]
mod tests {
    use super::map_intensity;

    #[test]
    fn maps_heat_levels() {
        assert_eq!(map_intensity(0, 100), 0);
        assert_eq!(map_intensity(10, 100), 1);
        assert_eq!(map_intensity(30, 100), 2);
        assert_eq!(map_intensity(60, 100), 3);
        assert_eq!(map_intensity(90, 100), 4);
    }
}
