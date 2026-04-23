use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::analytics::AnalyticsSnapshot;
use crate::analytics::model_stats::{ModelUsageRow, ProviderUsageRow, chart_with_focus};
use crate::ui::theme::Theme;
use crate::ui::widgets::common::{metric_line, truncate_label};
use crate::ui::widgets::linechart::build_chart;
use crate::utils::formatting::{format_price_summary, format_tokens};
use crate::utils::time::TimeRange;

pub fn render_models(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_model_index: usize,
    theme: &Theme,
) {
    let [chart_area, _, header_area, _, detail_area, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);

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
        Span::styled(format!("  ({:.2}%)", row.percentage), theme.muted_style()),
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
    let rows = layout_rows::<5, 2>(area);

    frame.render_widget(
        Paragraph::new(metric_line(
            "Total tokens: ",
            format_tokens(row.total_tokens),
            theme,
        )),
        rows[0][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Total cost: ",
            format_price_summary(&row.cost),
            theme,
        )),
        rows[0][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Input: ",
            format_tokens(row.input_tokens),
            theme,
        )),
        rows[1][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Sessions: ", row.sessions.to_string(), theme)),
        rows[1][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Output: ",
            format_tokens(row.output_tokens),
            theme,
        )),
        rows[2][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Messages: ", row.messages.to_string(), theme)),
        rows[2][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Cache: ",
            format_tokens(row.cache_tokens),
            theme,
        )),
        rows[3][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Prompts: ", row.prompts.to_string(), theme)),
        rows[3][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Rate: ",
            format!("{:.2} tok/s", row.p50_output_tokens_per_second),
            theme,
        )),
        rows[4][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Active days: ",
            row.active_days.to_string(),
            theme,
        )),
        rows[4][1],
    );
}

pub fn render_providers(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_provider_index: usize,
    theme: &Theme,
) {
    let [chart_area, _, header_area, _, detail_area, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);

    let focused_row = snapshot.providers.get(focused_provider_index);
    let chart_data = chart_with_focus(
        &snapshot.provider_chart,
        focused_row.map(|row| row.provider_id.as_str()),
    );
    frame.render_widget(build_chart(&chart_data, theme), chart_area);

    if let Some(row) = focused_row {
        frame.render_widget(
            Paragraph::new(focus_provider_line(
                row,
                focused_provider_index,
                &snapshot.providers,
                theme,
            )),
            header_area,
        );
        render_provider_detail(frame, detail_area, row, theme);
    } else {
        frame.render_widget(
            Paragraph::new("No provider activity in this time range.").style(theme.muted_style()),
            detail_area,
        );
    }
}

fn focus_provider_line(
    row: &ProviderUsageRow,
    focused_provider_index: usize,
    providers: &[ProviderUsageRow],
    theme: &Theme,
) -> Line<'static> {
    let total = providers.len().max(1);
    Line::from(vec![
        Span::styled(
            format!("  ● {}", truncate_label(&row.provider_id, 26)),
            Style::default().fg(theme.series_color(focused_provider_index)),
        ),
        Span::styled(format!("  ({:.2}%)", row.percentage), theme.muted_style()),
        Span::styled("  |  ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_provider_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled("  |  ", theme.muted_style()),
        Span::styled("j/k ↑/↓ cycle", theme.muted_style()),
    ])
}

fn render_provider_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    row: &ProviderUsageRow,
    theme: &Theme,
) {
    let rows = layout_rows::<5, 2>(area);

    frame.render_widget(
        Paragraph::new(metric_line(
            "Total tokens:",
            format_tokens(row.total_tokens),
            theme,
        )),
        rows[0][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Total cost: ",
            format_price_summary(&row.cost),
            theme,
        )),
        rows[0][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Input: ",
            format_tokens(row.input_tokens),
            theme,
        )),
        rows[1][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Sessions: ", row.sessions.to_string(), theme)),
        rows[1][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Output: ",
            format_tokens(row.output_tokens),
            theme,
        )),
        rows[2][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Messages: ", row.messages.to_string(), theme)),
        rows[2][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Cache: ",
            format_tokens(row.cache_tokens),
            theme,
        )),
        rows[3][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Prompts: ", row.prompts.to_string(), theme)),
        rows[3][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Rate: ",
            format!("{:.2} tok/s", row.p50_output_tokens_per_second),
            theme,
        )),
        rows[4][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Active days: ",
            row.active_days.to_string(),
            theme,
        )),
        rows[4][1],
    );
}

fn layout_rows<const ROW: usize, const COL: usize>(area: Rect) -> [[Rect; COL]; ROW] {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1); ROW])
        .areas::<ROW>(area)
        .map(|line| {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Fill(1); COL])
                .areas::<COL>(line)
        })
}
