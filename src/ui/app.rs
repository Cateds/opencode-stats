use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame, TerminalOptions, Viewport};
use tokio::sync::mpsc;

use crate::analytics::{build_snapshot, AnalyticsSnapshot};
use crate::cache::models_cache::{refresh_remote_models, PricingCatalog};
use crate::db::models::AppData;
use crate::ui::models::render_models;
use crate::ui::overview::render_overview;
use crate::ui::theme::{Theme, ThemeMode};
use crate::ui::widgets::common::{left_aligned_content, segment_span, CONTENT_WIDTH};
use crate::utils::time::TimeRange;

const VIEWPORT_HEIGHT: u16 = 23;
const STATUS_TTL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
struct StatusMessage {
    text: String,
    expires_at: Instant,
}

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
    status_message: Option<StatusMessage>,
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
        Self::restore(&mut terminal);
        app_result
    }

    /// 默认的 ratatui::restore 在 Inline Viewport 下有错误的行为，
    /// 此处重置终端以确保光标和输入状态正确恢复
    fn restore(terminal: &mut DefaultTerminal) {
        terminal.clear().unwrap();
        crossterm::terminal::disable_raw_mode().unwrap();
        print_exit_art();
    }

    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            self.clear_expired_status();
            while let Ok(pricing) = self.pricing_updates.try_recv() {
                self.pricing = pricing;
                self.recompute();
                self.set_status("Pricing cache refreshed from models.dev");
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
            segment_span(" All ", self.range == TimeRange::All, theme),
            ratatui::text::Span::raw(" "),
            segment_span("7 Days", self.range == TimeRange::Last7Days, theme),
            ratatui::text::Span::raw(" "),
            segment_span("30 Days", self.range == TimeRange::Last30Days, theme),
            ratatui::text::Span::raw("     "),
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
            .as_ref()
            .map(|status| status.text.as_str())
            .unwrap_or("tab/←/→/h/l pages | r cycle | 1/2/3 pick | <ctrl-s> copy | q exit");
        frame.render_widget(
            ratatui::widgets::Paragraph::new(status).style(theme.muted_style()),
            left_aligned_content(area),
        );
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.page = self.page.next(),
            KeyCode::Left | KeyCode::Char('h') => self.page = self.page.previous(),
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

    fn set_status(&mut self, text: impl Into<String>) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            expires_at: Instant::now() + STATUS_TTL,
        });
    }

    fn clear_expired_status(&mut self) {
        if self
            .status_message
            .as_ref()
            .is_some_and(|status| Instant::now() >= status.expires_at)
        {
            self.status_message = None;
        }
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
            Ok(()) => self.set_status("Copied current page summary to clipboard"),
            Err(err) => self.set_status(format!("Clipboard unavailable: {err}")),
        }
    }
}

fn print_exit_art() {
    const ART: &str = r#"
  ███████╗████████╗ █████╗ ████████╗███████╗
  ██╔════╝╚══██╔══╝██╔══██╗╚══██╔══╝██╔════╝
  ███████╗   ██║   ███████║   ██║   ███████╗
  ╚════██║   ██║   ██╔══██║   ██║   ╚════██║
  ███████║   ██║   ██║  ██║   ██║   ███████║
  ╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚══════╝
"#;
    eprintln!("{ART}");
}
