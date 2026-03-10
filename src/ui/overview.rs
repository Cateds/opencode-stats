use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::analytics::AnalyticsSnapshot;
use crate::ui::theme::Theme;
use crate::ui::widgets::common::{left_aligned_content, metric_line};
use crate::ui::widgets::heatmap::HeatmapWidget;
use crate::utils::formatting::{format_tokens, format_usd_precise};

pub fn render_overview(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: crate::utils::time::TimeRange,
    theme: &Theme,
) {
    let content = left_aligned_content(area);
    let [heatmap, legend, spacer, favorite, stats, fun] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .areas(content);

    frame.render_widget(
        HeatmapWidget {
            data: &snapshot.heatmap,
            theme,
        },
        heatmap,
    );

    frame.render_widget(
        Paragraph::new(crate::ui::widgets::heatmap::heatmap_legend_line(theme)),
        legend,
    );
    frame.render_widget(Paragraph::new(""), spacer);

    let favorite_model = snapshot
        .models
        .first()
        .map(|row| row.model_id.clone())
        .unwrap_or_else(|| "n/a".to_string());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Favorite model: ", theme.muted_style()),
            Span::styled(favorite_model, theme.accent_style()),
        ])),
        favorite,
    );

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .areas(stats);

    let left_text = Text::from(vec![
        metric_line(
            "Total tokens: ",
            format_tokens(snapshot.overview.total_tokens),
            theme,
        ),
        metric_line(
            "Input: ",
            format_tokens(snapshot.overview.input_tokens),
            theme,
        ),
        metric_line(
            "Output: ",
            format_tokens(snapshot.overview.output_tokens),
            theme,
        ),
        metric_line(
            "Cache: ",
            format_tokens(snapshot.overview.cache_tokens),
            theme,
        ),
    ]);
    frame.render_widget(Paragraph::new(left_text), left);

    let right_text = Text::from(vec![
        metric_line(
            "Total cost: ",
            format_usd_precise(snapshot.overview.total_cost),
            theme,
        ),
        metric_line(
            "Interactions: ",
            snapshot.overview.interactions.to_string(),
            theme,
        ),
        metric_line(
            "Models used: ",
            snapshot.overview.models_used.to_string(),
            theme,
        ),
        metric_line(
            "Active days: ",
            snapshot.overview.active_days.to_string(),
            theme,
        ),
    ]);
    frame.render_widget(Paragraph::new(right_text), right);

    let comparison = split_comparison_lines(&snapshot.overview.fun_comparison)
        .into_iter()
        .map(|line| Line::from(Span::styled(line, theme.comparison_style())))
        .collect::<Vec<_>>();
    let fun_text = Paragraph::new(Text::from(comparison));
    frame.render_widget(fun_text, fun);
}

fn split_comparison_lines(text: &str) -> Vec<String> {
    if let Some((left, right)) = text.split_once(", ") {
        vec![format!("{left},"), format!("{}{right}", " ".repeat(12))]
    } else {
        vec![text.to_string()]
    }
}
