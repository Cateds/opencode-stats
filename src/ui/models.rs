use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::analytics::AnalyticsSnapshot;
use crate::analytics::model_stats::{ModelUsageRow, ProviderUsageRow, chart_with_focus};
use crate::ui::theme::Theme;
use crate::ui::widgets::common::{metric_line, truncate_label};
use crate::ui::widgets::linechart::build_chart;
use crate::utils::formatting::{format_price_summary, format_tokens};
use crate::utils::time::TimeRange;

#[derive(Clone, Debug)]
pub struct SearchState {
    pub query: String,
    pub ids: Vec<String>,
    pub filtered_indices: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl SearchState {
    pub fn new(ids: Vec<String>, focused_index: usize) -> Self {
        let total = ids.len();
        let selected = focused_index.min(total.saturating_sub(1));
        let scroll_offset = if selected > 4 { selected - 4 } else { 0 };
        Self {
            query: String::new(),
            ids,
            filtered_indices: (0..total).collect(),
            selected,
            scroll_offset,
        }
    }

    pub fn update_filter(&mut self) {
        let has_query = !self.query.trim().is_empty();
        self.filtered_indices = filter_indices(&self.query, &self.ids);
        if has_query {
            self.selected = 0;
            self.scroll_offset = 0;
        } else {
            self.selected = self
                .selected
                .min(self.filtered_indices.len().saturating_sub(1));
            let filtered_total = self.filtered_indices.len();
            self.scroll_offset = self
                .scroll_offset
                .min(filtered_total.saturating_sub(5));
            if self.scroll_offset > self.selected {
                self.scroll_offset = self.selected;
            }
        }
    }
}

fn filter_indices(query: &str, items: &[String]) -> Vec<usize> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (0..items.len()).collect();
    }

    let mut matcher = Matcher::default();
    let terms: Vec<&str> = trimmed.split_whitespace().collect();
    let mut results: Vec<(usize, u32)> = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let mut char_buf = Vec::new();
        let haystack = nucleo_matcher::Utf32Str::new(item, &mut char_buf);
        let mut all_matched = true;
        let mut total_score = 0u32;

        for term in &terms {
            let pattern = Pattern::parse(term, CaseMatching::Ignore, Normalization::Smart);
            if let Some(score) = pattern.score(haystack, &mut matcher) {
                total_score += score;
            } else {
                all_matched = false;
                break;
            }
        }

        if all_matched {
            results.push((i, total_score));
        }
    }

    results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    results.into_iter().map(|(i, _)| i).collect()
}

fn fuzzy_match_positions(query: &str, text: &str) -> Vec<usize> {
    let mut matcher = Matcher::default();
    let terms: Vec<&str> = query.split_whitespace().collect();
    let mut char_buf = Vec::new();
    let haystack = nucleo_matcher::Utf32Str::new(text, &mut char_buf);
    let mut positions: Vec<usize> = Vec::new();

    for term in &terms {
        let pattern = Pattern::parse(term, CaseMatching::Ignore, Normalization::Smart);
        let mut indices_buf = Vec::new();
        pattern.indices(haystack, &mut matcher, &mut indices_buf);
        if !indices_buf.is_empty() {
            for idx in indices_buf {
                positions.push(idx as usize);
            }
        }
    }

    positions.sort_unstable();
    positions.dedup();
    positions
}

fn merge_to_ranges(positions: &[usize]) -> Vec<(usize, usize)> {
    if positions.is_empty() {
        return vec![];
    }
    let mut ranges = Vec::new();
    let mut start = positions[0];
    let mut end = start + 1;
    for &pos in &positions[1..] {
        if pos == end {
            end = pos + 1;
        } else {
            ranges.push((start, end));
            start = pos;
            end = pos + 1;
        }
    }
    ranges.push((start, end));
    ranges
}

fn truncate_query_left(query: &str, max_width: u16) -> String {
    let full_w = UnicodeWidthStr::width(query) as u16;
    if full_w <= max_width {
        return query.to_string();
    }
    let avail = max_width.saturating_sub(1);
    let chars: Vec<char> = query.chars().collect();
    let mut w = 0u16;
    let mut i = chars.len();
    while i > 0 {
        i -= 1;
        w += UnicodeWidthChar::width(chars[i]).unwrap_or(0) as u16;
        if w > avail {
            i += 1;
            break;
        }
    }
    let mut result = String::from("…");
    result.extend(&chars[i..]);
    result
}

pub const MAX_QUERY_LEN: usize = 256;

pub fn render_models(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AnalyticsSnapshot,
    _range: TimeRange,
    focused_model_index: usize,
    search: Option<&SearchState>,
    theme: &Theme,
) {
    let [chart_area, spacer1, header_area, spacer2, detail_area, _] = Layout::default()
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

    let effective_focus: Option<usize> = search
        .and_then(|s| s.filtered_indices.get(s.selected).copied())
        .or(Some(focused_model_index));

    let focused_row = effective_focus.and_then(|i| snapshot.models.get(i));
    let chart_data = chart_with_focus(
        &snapshot.chart,
        focused_row.map(|row| row.model_id.as_str()),
    );
    frame.render_widget(build_chart(&chart_data, theme), chart_area);

    if let Some(search) = search {
        frame.render_widget(Paragraph::new(""), spacer1);
        render_search_overlay(
            frame,
            header_area,
            spacer2,
            detail_area,
            search,
            &snapshot.models,
            theme,
        );
    } else if let Some(row) = focused_row {
        frame.render_widget(
            Paragraph::new(focus_header_line(
                row,
                focused_model_index,
                &snapshot.models,
                theme,
            )),
            header_area,
        );
        frame.render_widget(Paragraph::new(""), spacer2);
        render_model_detail(frame, detail_area, row, theme);
    } else {
        frame.render_widget(Paragraph::new(""), spacer2);
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
        Span::styled(" | ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_model_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("j/k ↑/↓", theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("f find", theme.muted_style()),
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
    search: Option<&SearchState>,
    theme: &Theme,
) {
    let [chart_area, spacer1, header_area, spacer2, detail_area, _] = Layout::default()
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

    let effective_focus: Option<usize> = search
        .and_then(|s| s.filtered_indices.get(s.selected).copied())
        .or(Some(focused_provider_index));

    let focused_row = effective_focus.and_then(|i| snapshot.providers.get(i));
    let chart_data = chart_with_focus(
        &snapshot.provider_chart,
        focused_row.map(|row| row.provider_id.as_str()),
    );
    frame.render_widget(build_chart(&chart_data, theme), chart_area);

    if let Some(search) = search {
        frame.render_widget(Paragraph::new(""), spacer1);
        render_search_overlay(
            frame,
            header_area,
            spacer2,
            detail_area,
            search,
            &snapshot.providers,
            theme,
        );
    } else if let Some(row) = focused_row {
        frame.render_widget(
            Paragraph::new(focus_provider_line(
                row,
                focused_provider_index,
                &snapshot.providers,
                theme,
            )),
            header_area,
        );
        frame.render_widget(Paragraph::new(""), spacer2);
        render_provider_detail(frame, detail_area, row, theme);
    } else {
        frame.render_widget(Paragraph::new(""), spacer2);
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
        Span::styled(" | ", theme.muted_style()),
        Span::styled(
            format!("{}/{}", focused_provider_index.min(total - 1) + 1, total),
            theme.muted_style(),
        ),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("j/k ↑/↓", theme.muted_style()),
        Span::styled(" | ", theme.muted_style()),
        Span::styled("f find", theme.muted_style()),
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

trait SearchItem {
    fn item_id(&self) -> &str;
    fn item_pct(&self) -> f64;
}

impl SearchItem for ModelUsageRow {
    fn item_id(&self) -> &str {
        &self.model_id
    }
    fn item_pct(&self) -> f64 {
        self.percentage
    }
}

impl SearchItem for ProviderUsageRow {
    fn item_id(&self) -> &str {
        &self.provider_id
    }
    fn item_pct(&self) -> f64 {
        self.percentage
    }
}

fn render_search_overlay<T: SearchItem>(
    frame: &mut ratatui::Frame<'_>,
    header_area: Rect,
    spacer_area: Rect,
    detail_area: Rect,
    search: &SearchState,
    items: &[T],
    theme: &Theme,
) {
    let filtered_total = search.filtered_indices.len();
    let visible_start = search.scroll_offset;
    let visible_end = (visible_start + 5).min(filtered_total);
    let visible_slice = &search.filtered_indices[visible_start..visible_end];

    let hint = format!(
        "{}/{} | ↑/↓ | <esc> quit | <enter> select",
        filtered_total,
        items.len(),
    );
    let hint_width = UnicodeWidthStr::width(hint.as_str()) as u16;
    let [input_area, hint_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(hint_width)])
            .areas::<2>(header_area);

    let input_prefix = "  ● ";
    let prefix_w = UnicodeWidthStr::width(input_prefix) as u16;
    let max_input_w = input_area
        .width
        .saturating_sub(prefix_w)
        .saturating_sub(1);
    let display_query = truncate_query_left(&search.query, max_input_w);
    let input_line = Line::from(vec![
        Span::styled(input_prefix, theme.muted_style()),
        Span::styled(format!("{}_", display_query), Style::default().fg(theme.foreground)),
    ]);
    frame.render_widget(Paragraph::new(input_line), input_area);
    frame.render_widget(
        Paragraph::new(Span::styled(hint, theme.muted_style())),
        hint_area,
    );
    frame.render_widget(Paragraph::new(""), spacer_area);

    for line in 0..5 {
        let row_y = detail_area.y + line as u16;
        let row = Rect {
            x: detail_area.x,
            y: row_y,
            width: detail_area.width,
            height: 1,
        };

        let [indent, text_area, sb_area] = Layout::horizontal([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas::<3>(row);

        if line < visible_slice.len() {
            let real_idx = visible_slice[line];
            let item = &items[real_idx];
            let id = item.item_id();
            let pct = item.item_pct();
            let is_selected = (visible_start + line) == search.selected;

            let indicator_style = if is_selected {
                theme.accent_style()
            } else {
                theme.muted_style()
            };
            frame.render_widget(
                Paragraph::new(Span::styled("▌", indicator_style)),
                Rect {
                    x: indent.x + 2,
                    y: indent.y,
                    width: 1,
                    height: 1,
                },
            );

            let mut spans: Vec<Span> = Vec::new();
            let fg = Style::default().fg(theme.foreground);
            if search.query.trim().is_empty() {
                spans.push(Span::styled(id.to_string(), fg));
            } else {
                let positions = fuzzy_match_positions(&search.query, id);
                if positions.is_empty() {
                    spans.push(Span::styled(id.to_string(), fg));
                } else {
                    let ranges = merge_to_ranges(&positions);
                    let mut cursor = 0usize;
                    for (start, end) in &ranges {
                        if *start > cursor {
                            spans.push(Span::styled(id[cursor..*start].to_string(), fg));
                        }
                        spans.push(Span::styled(&id[*start..*end], theme.accent_style()));
                        cursor = *end;
                    }
                    if cursor < id.len() {
                        spans.push(Span::styled(id[cursor..].to_string(), fg));
                    }
                }
            }
            spans.push(Span::styled(format!(" ({:.2}%)", pct), theme.muted_style()));

            let text_rect = Rect {
                x: text_area.x + 1,
                y: text_area.y,
                width: text_area.width.saturating_sub(1),
                height: 1,
            };
            frame.render_widget(Paragraph::new(Line::from(spans)), text_rect);
        }

        if filtered_total > 5 {
            let thumb_size = (5.0 * 5.0 / filtered_total as f64).max(1.0) as usize;
            let thumb_start = ((search.scroll_offset as f64 / (filtered_total - 5) as f64)
                * (5.0 - thumb_size as f64)) as usize;
            let ch = if line >= thumb_start && line < thumb_start + thumb_size {
                "┃"
            } else {
                " "
            };
            frame.render_widget(
                Paragraph::new(Span::styled(ch, theme.muted_style())),
                sb_area,
            );
        }
    }
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
