use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

pub const CONTENT_WIDTH: u16 = 70;

pub fn left_aligned_content(area: Rect) -> Rect {
    let width = CONTENT_WIDTH.min(area.width);
    Rect {
        x: area.x,
        y: area.y,
        width,
        height: area.height,
    }
}

pub fn segment_span(label: &str, active: bool, theme: &Theme) -> Span<'static> {
    let style = if active {
        Style::default()
            .fg(theme.tab_active_fg)
            .bg(theme.tab_active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.muted_style()
    };

    Span::styled(format!(" {} ", label), style)
}

pub fn truncate_label(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    let mut truncated = value.chars().take(keep).collect::<String>();
    truncated.push_str("...");
    truncated
}

pub fn metric_line<'a>(label: &'a str, value: String, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(label.to_string(), theme.muted_style()),
        Span::raw(value),
    ])
}
