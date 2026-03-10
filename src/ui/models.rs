use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::analytics::model_stats::{chart_with_focus, ModelUsageRow};
use crate::analytics::AnalyticsSnapshot;
use crate::ui::theme::Theme;
use crate::ui::widgets::common::{left_aligned_content, metric_line, truncate_label};
use crate::ui::widgets::linechart::build_chart;
use crate::utils::formatting::{format_tokens, format_usd_precise};
use crate::utils::time::TimeRange;

pub fn render_models(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_model_index: usize,
    theme: &Theme,
) {
    let content = left_aligned_content(area);
    let [chart_area, _, header_area, _, detail_area, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .areas(content);

    let focused_row = snapshot.models.get(focused_model_index);
    let chart_data = chart_with_focus(
        &snapshot.chart,
        focused_row.map(|row| row.model_id.as_str()),
    );
    frame.render_widget(build_chart(&chart_data, theme), chart_area);

    if let Some(row) = focused_row {
        frame.render_widget(
            Paragraph::new(focus_header_line(
                row,
                focused_model_index,
                &snapshot.models,
                theme,
            )),
            header_area,
        );
        render_model_detail(frame, detail_area, row, theme);
    } else {
        frame.render_widget(
            Paragraph::new("No model activity in this time range.").style(theme.muted_style()),
            detail_area,
        );
    }
}

fn focus_header_line(
    row: &ModelUsageRow,
    focused_model_index: usize,
    models: &[ModelUsageRow],
    theme: &Theme,
) -> Line<'static> {
    let total = models.len().max(1);
    Line::from(vec![
        Span::styled(
            format!("  ● {}", truncate_label(&row.model_id, 26)),
            Style::default().fg(theme.series_color(focused_model_index)),
        ),
        Span::styled(format!("  ({:.1}%)", row.percentage), theme.muted_style()),
        Span::styled("  |  ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_model_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled("  |  ", theme.muted_style()),
        Span::styled("j/k ↑/↓ cycle", theme.muted_style()),
    ])
}

fn render_model_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    row: &ModelUsageRow,
    theme: &Theme,
) {
    let [top_left, top_mid, top_right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .areas(Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        });
    let [bottom_left, bottom_mid, bottom_right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .areas(Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        });

    frame.render_widget(
        Paragraph::new(metric_line(
            "Tokens ",
            format_tokens(row.total_tokens),
            theme,
        )),
        top_left,
    );
    frame.render_widget(
        Paragraph::new(metric_line("Cost ", format_usd_precise(row.cost), theme)),
        top_mid,
    );
    frame.render_widget(
        Paragraph::new(metric_line("Sessions ", row.sessions.to_string(), theme)),
        top_right,
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Interactions ",
            row.interactions.to_string(),
            theme,
        )),
        bottom_left,
    );
    frame.render_widget(
        Paragraph::new(metric_line("Days ", row.active_days.to_string(), theme)),
        bottom_mid,
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Rate ",
            format!("{:.1} tok/s", row.p50_output_tokens_per_second),
            theme,
        )),
        bottom_right,
    );
}
