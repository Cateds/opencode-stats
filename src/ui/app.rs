use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame, TerminalOptions, Viewport};
use tokio::sync::mpsc;

use crate::analytics::{AnalyticsSnapshot, build_snapshot};
use crate::cache::models_cache::{PricingCatalog, refresh_remote_models};
use crate::db::models::AppData;
use crate::ui::models::render_models;
use crate::ui::overview::render_overview;
use crate::ui::theme::{Theme, ThemeMode};
use crate::ui::widgets::common::{CONTENT_WIDTH, left_aligned_content, segment_span};
use crate::utils::time::TimeRange;

const VIEWPORT_HEIGHT: u16 = 23;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Overview,
    Models,
}

impl Page {
    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Models,
            Self::Models => Self::Overview,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

pub struct App {
    pub data: AppData,
    pub pricing: PricingCatalog,
    pub snapshot: AnalyticsSnapshot,
    pub page: Page,
    pub range: TimeRange,
    pub theme_mode: ThemeMode,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub focused_model_index: usize,
    pricing_updates: mpsc::UnboundedReceiver<PricingCatalog>,
}

impl App {
    pub fn new(data: AppData, pricing: PricingCatalog, theme_mode: ThemeMode) -> Self {
        let snapshot = build_snapshot(&data, &pricing, TimeRange::All);
        let (sender, receiver) = mpsc::unbounded_channel();
        if pricing.refresh_needed {
            let cache_path = pricing.cache_path.clone();
            tokio::spawn(refresh_remote_models(cache_path, sender));
        }

        Self {
            data,
            pricing,
            snapshot,
            page: Page::Overview,
            range: TimeRange::All,
            theme_mode,
            should_quit: false,
            status_message: None,
            focused_model_index: 0,
            pricing_updates: receiver,
        }
    }

    pub fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_HEIGHT),
        });

        let app_result = self.run_loop(&mut terminal);
        ratatui::restore();
        app_result
    }

    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            while let Ok(pricing) = self.pricing_updates.try_recv() {
                self.pricing = pricing;
                self.recompute();
                self.status_message = Some("Pricing cache refreshed from models.dev".to_string());
            }

            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key);
                    }
                }
            }
        }
        Ok(())
    }

    fn recompute(&mut self) {
        self.snapshot = build_snapshot(&self.data, &self.pricing, self.range);
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let theme = Theme::from_mode(self.theme_mode);
        let area = frame.area();
        let vertical = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(16),
            ratatui::layout::Constraint::Length(1),
        ]);
        let [divider, header, spacer, body, footer] = vertical.areas(area);

        frame.render_widget(
            ratatui::widgets::Paragraph::new("─".repeat(CONTENT_WIDTH as _))
                .style(theme.muted_style()),
            divider,
        );

        self.render_header(frame, header, &theme);
        frame.render_widget(ratatui::widgets::Paragraph::new(""), spacer);

        match self.page {
            Page::Overview => render_overview(frame, body, &self.snapshot, self.range, &theme),
            Page::Models => render_models(
                frame,
                body,
                &self.snapshot,
                self.range,
                self.focused_model_index,
                &theme,
            ),
        }

        self.render_footer(frame, footer, &theme);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect, theme: &Theme) {
        let content = left_aligned_content(area);
        let line = ratatui::text::Line::from(vec![
            segment_span("Overview", self.page == Page::Overview, theme),
            ratatui::text::Span::raw(" "),
            segment_span("Models", self.page == Page::Models, theme),
            ratatui::text::Span::raw("     "),
            segment_span("All", self.range == TimeRange::All, theme),
            ratatui::text::Span::raw(" "),
            segment_span("7 Days", self.range == TimeRange::Last7Days, theme),
            ratatui::text::Span::raw(" "),
            segment_span("30 Days", self.range == TimeRange::Last30Days, theme),
            ratatui::text::Span::raw("    "),
            ratatui::text::Span::styled(
                format!("Source: {:?}", self.data.source),
                theme.muted_style(),
            ),
        ]);

        frame.render_widget(ratatui::widgets::Paragraph::new(line), content);
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect, theme: &Theme) {
        let status = self
            .status_message
            .as_deref()
            .unwrap_or("tab/←/→ pages | r cycle | 1/2/3 pick | <ctrl-s> copy | q exit");
        frame.render_widget(
            ratatui::widgets::Paragraph::new(status).style(theme.muted_style()),
            left_aligned_content(area),
        );
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::Right => self.page = self.page.next(),
            KeyCode::Left => self.page = self.page.previous(),
            KeyCode::Down | KeyCode::Char('j') => self.advance_focused_model(1),
            KeyCode::Up | KeyCode::Char('k') => self.advance_focused_model(-1),
            KeyCode::Char('r') => {
                self.range = self.range.cycle();
                self.focused_model_index = 0;
                self.recompute();
            }
            KeyCode::Char(value)
                if key.modifiers.contains(KeyModifiers::CONTROL) && value == 's' =>
            {
                self.copy_summary();
            }
            KeyCode::Char(value) => {
                if let Some(range) = TimeRange::from_shortcut(value) {
                    self.range = range;
                    self.focused_model_index = 0;
                    self.recompute();
                }
            }
            _ => {}
        }
    }

    fn advance_focused_model(&mut self, delta: isize) {
        if self.page != Page::Models || self.snapshot.models.is_empty() {
            return;
        }

        let current = self.focused_model_index as isize;
        let total = self.snapshot.models.len() as isize;
        let next = (current + delta).rem_euclid(total) as usize;
        self.focused_model_index = next;
    }

    fn copy_summary(&mut self) {
        let summary = match self.page {
            Page::Overview => format!(
                "oc-stats {}\nTokens: {}\nCost: {}\nSessions: {}\nInteractions: {}",
                self.range.label(),
                self.snapshot.overview.total_tokens,
                self.snapshot.overview.total_cost,
                self.snapshot.overview.sessions,
                self.snapshot.overview.interactions,
            ),
            Page::Models => self
                .snapshot
                .models
                .iter()
                .take(8)
                .map(|row| {
                    format!(
                        "{}: {} tokens ({:.1}%)",
                        row.model_id, row.total_tokens, row.percentage
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(summary)) {
            Ok(()) => {
                self.status_message = Some("Copied current page summary to clipboard".to_string())
            }
            Err(err) => self.status_message = Some(format!("Clipboard unavailable: {err}")),
        }
    }
}
