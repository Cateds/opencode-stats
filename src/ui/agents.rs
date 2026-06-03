use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::analytics::AnalyticsSnapshot;
use crate::analytics::agent_stats::{AgentModelBreakdown, AgentUsageRow};
use crate::analytics::model_stats::chart_with_focus;
use crate::ui::app::AgentChartMode;
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

#[allow(clippy::too_many_arguments)]
pub fn render_agents(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_agent_index: usize,
    chart_mode: AgentChartMode,
    focused_model_index: usize,
    search: Option<&SearchState>,
    theme: &Theme,
) {
    let [chart_area, spacer1, header_area, spacer2, detail_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
        ])
        .areas(area);

    let effective_focus: Option<usize> = search
        .and_then(|s| s.filtered_indices.get(s.selected).copied())
        .or(Some(focused_agent_index));

    let focused_row = effective_focus.and_then(|i| snapshot.agents.get(i));
    let focused_model = focused_row.and_then(|row| row.model_breakdown.get(focused_model_index));

    let chart_data = match chart_mode {
        AgentChartMode::AllAgents => chart_with_focus(
            &snapshot.agent_chart,
            focused_row.map(|row| row.agent_id.as_str()),
        ),
        AgentChartMode::PerModel => match focused_row {
            Some(row) => {
                let highlight = focused_model.map(|m| m.model_id.as_str());
                snapshot
                    .agent_model_charts
                    .iter()
                    .find(|(id, _)| id == &row.agent_id)
                    .map(|(_, chart)| chart_with_focus(chart, highlight))
                    .unwrap_or_else(|| chart_with_focus(&snapshot.agent_chart, None))
            }
            None => chart_with_focus(&snapshot.agent_chart, None),
        },
    };
    frame.render_widget(build_chart(&chart_data, theme), chart_area);
    frame.render_widget(
        Paragraph::new(mode_indicator(chart_mode, focused_row, theme)),
        spacer1,
    );

    if let Some(search) = search {
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
        match chart_mode {
            AgentChartMode::AllAgents => {
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
            }
            AgentChartMode::PerModel => {
                if let Some(model) = focused_model {
                    frame.render_widget(
                        Paragraph::new(focus_model_line(model, focused_model_index, row, theme)),
                        header_area,
                    );
                    frame.render_widget(Paragraph::new(""), spacer2);
                    render_model_detail(frame, detail_area, model, theme);
                } else {
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
                    frame.render_widget(
                        Paragraph::new("No model data for this agent.").style(theme.muted_style()),
                        detail_area,
                    );
                }
            }
        }
    } else {
        frame.render_widget(Paragraph::new(""), spacer2);
        frame.render_widget(
            Paragraph::new("No agent activity in this time range.").style(theme.muted_style()),
            detail_area,
        );
    }
}

fn mode_indicator(
    chart_mode: AgentChartMode,
    focused_row: Option<&AgentUsageRow>,
    theme: &Theme,
) -> Line<'static> {
    let text = match chart_mode {
        AgentChartMode::AllAgents => "  All agents".to_string(),
        AgentChartMode::PerModel => match focused_row {
            Some(agent) => {
                format!("  Models used by {}", truncate_label(&agent.agent_id, 40),)
            }
            None => "  Per-model (no data)".to_string(),
        },
    };
    Line::from(Span::styled(text, theme.muted_style()))
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
        Span::styled(" | ", theme.muted_style()),
        Span::styled("m chart", theme.muted_style()),
    ])
}

fn focus_model_line(
    model: &AgentModelBreakdown,
    focused_model_index: usize,
    agent_row: &AgentUsageRow,
    theme: &Theme,
) -> Line<'static> {
    let total = agent_row.model_breakdown.len().max(1);
    let pct = if agent_row.total_tokens > 0 {
        (model.tokens as f64 / agent_row.total_tokens as f64) * 100.0
    } else {
        0.0
    };
    Line::from(vec![
        Span::styled(
            format!("  ● {}", truncate_label(&model.model_id, 26)),
            Style::default().fg(theme.series_color(focused_model_index)),
        ),
        Span::styled(format!("  ({:.2}%)", pct), theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_model_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("j/k ↑/↓", theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("f find", theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("m chart", theme.muted_style()),
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

fn render_model_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &AgentModelBreakdown,
    theme: &Theme,
) {
    let rows = layout_rows::<4, 2>(area);

    frame.render_widget(
        Paragraph::new(metric_line(
            "Total tokens: ",
            format_tokens(model.tokens),
            theme,
        )),
        rows[0][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Total cost: ",
            format_price_summary(&model.cost),
            theme,
        )),
        rows[0][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Input: ",
            format_tokens(model.input_tokens),
            theme,
        )),
        rows[1][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line("Sessions: ", model.sessions.to_string(), theme)),
        rows[1][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Output: ",
            format_tokens(model.output_tokens),
            theme,
        )),
        rows[2][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Active days: ",
            model.active_days.to_string(),
            theme,
        )),
        rows[2][1],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Cache: ",
            format_tokens(model.cache_tokens),
            theme,
        )),
        rows[3][0],
    );
    frame.render_widget(
        Paragraph::new(metric_line(
            "Rate: ",
            format!("{:.2} tok/s", model.p50_output_tokens_per_second),
            theme,
        )),
        rows[3][1],
    );
}
