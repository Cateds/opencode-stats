use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::analytics::AnalyticsSnapshot;
use crate::analytics::agent_stats::AgentUsageRow;
use crate::analytics::model_stats::chart_with_focus;
use crate::ui::models::{SearchItem, SearchState, layout_rows};
use crate::ui::theme::Theme;
use crate::ui::widgets::common::{metric_line, truncate_label};
use crate::ui::widgets::linechart::build_chart;
use crate::utils::formatting::{format_price_summary, format_tokens};
use crate::utils::time::TimeRange;

impl SearchItem for AgentUsageRow {
    fn item_id(&self) -> &str {
        &self.agent_id
    }
    fn item_pct(&self) -> f64 {
        self.percentage
    }
}

pub fn render_agents(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_agent_index: usize,
    search: Option<&SearchState>,
    theme: &Theme,
) {
    let [
        chart_area,
        spacer1,
        header_area,
        spacer2,
        detail_area,
        model_area,
    ] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .areas(area);

    let effective_focus: Option<usize> = search
        .and_then(|s| s.filtered_indices.get(s.selected).copied())
        .or(Some(focused_agent_index));

    let focused_row = effective_focus.and_then(|i| snapshot.agents.get(i));
    let chart_data = chart_with_focus(
        &snapshot.agent_chart,
        focused_row.map(|row| row.agent_id.as_str()),
    );
    frame.render_widget(build_chart(&chart_data, theme), chart_area);

    if let Some(search) = search {
        frame.render_widget(Paragraph::new(""), spacer1);
        super::models::render_search_overlay(
            frame,
            header_area,
            spacer2,
            detail_area,
            search,
            &snapshot.agents,
            theme,
        );
    } else if let Some(row) = focused_row {
        frame.render_widget(
            Paragraph::new(focus_agent_line(
                row,
                focused_agent_index,
                &snapshot.agents,
                theme,
            )),
            header_area,
        );
        frame.render_widget(Paragraph::new(""), spacer2);
        render_agent_detail(frame, detail_area, row, theme);
        render_model_breakdown(frame, model_area, row, theme);
    } else {
        frame.render_widget(Paragraph::new(""), spacer2);
        frame.render_widget(
            Paragraph::new("No agent activity in this time range.").style(theme.muted_style()),
            detail_area,
        );
    }
}

fn focus_agent_line(
    row: &AgentUsageRow,
    focused_agent_index: usize,
    agents: &[AgentUsageRow],
    theme: &Theme,
) -> Line<'static> {
    let total = agents.len().max(1);
    Line::from(vec![
        Span::styled(
            format!("  ● {}", truncate_label(&row.agent_id, 26)),
            Style::default().fg(theme.series_color(focused_agent_index)),
        ),
        Span::styled(format!("  ({:.2}%)", row.percentage), theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_agent_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("j/k ↑/↓", theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("f find", theme.muted_style()),
    ])
}

fn render_agent_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    row: &AgentUsageRow,
    theme: &Theme,
) {
    let rows = layout_rows::<4, 2>(area);

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
        Paragraph::new(metric_line(
            "Active days: ",
            row.active_days.to_string(),
            theme,
        )),
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
        Paragraph::new(metric_line(
            "Rate: ",
            format!("{:.2} tok/s", row.p50_output_tokens_per_second),
            theme,
        )),
        rows[3][1],
    );
}

fn render_model_breakdown(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    row: &AgentUsageRow,
    theme: &Theme,
) {
    let models = &row.model_breakdown;
    let available = area.height as usize;

    if models.is_empty() || available == 0 {
        return;
    }

    let show_count = available.min(models.len());

    let constraints: Vec<Constraint> = (0..show_count).map(|_| Constraint::Length(1)).collect();
    let lines = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, line_area) in lines.iter().enumerate() {
        if i >= models.len() {
            break;
        }
        let m = &models[i];

        let label = crate::ui::widgets::common::truncate_label(&m.model_id, 20);
        let pct = if row.total_tokens > 0 {
            (m.tokens as f64 / row.total_tokens as f64) * 100.0
        } else {
            0.0
        };
        let tokens = format_tokens(m.tokens);
        let cost = format_price_summary(&m.cost);

        let model_line = Line::from(vec![
            Span::styled("  · ", theme.muted_style()),
            Span::styled(label, Style::default().fg(theme.foreground)),
            Span::styled(format!(": {tokens} ({pct:.1}%)"), theme.muted_style()),
            Span::styled(format!(" | sessions: {}", m.sessions), theme.muted_style()),
            Span::styled(format!(" | {cost}"), theme.muted_style()),
        ]);

        frame.render_widget(Paragraph::new(model_line), *line_area);
    }
}
