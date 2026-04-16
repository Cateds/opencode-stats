use std::borrow::Cow;
use std::time::{Duration, Instant};

use anyhow::Result;
use colored::{ColoredString, Colorize};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Padding};
use ratatui::{DefaultTerminal, Frame, Terminal, TerminalOptions, Viewport, backend::TestBackend};
use tokio::sync::mpsc;

use crate::analytics::{AnalyticsSnapshot, build_snapshot};
use crate::cache::models_cache::{PricingCatalog, refresh_remote_models};
use crate::db::models::AppData;
use crate::ui::export::render_share_card;
use crate::ui::models::{render_models, render_providers};
use crate::ui::overview::render_overview;
use crate::ui::theme::{Theme, ThemeKind};
use crate::ui::widgets::common::{CONTENT_WIDTH, left_aligned_content, segment_span};
use crate::utils::formatting::format_price_summary;
use crate::utils::pricing::ZeroCostBehavior;
use crate::utils::time::TimeRange;

const VIEWPORT_HEIGHT: u16 = 23;
const STATUS_TTL: Duration = Duration::from_secs(1);
const TICK_TIME_MS: u16 = 200;

#[derive(Clone, Debug)]
struct StatusMessage {
    text: String,
    expires_at: Option<Instant>,
}

#[derive(Debug)]
struct ClipboardJob {
    buffer: ratatui::buffer::Buffer,
    theme: Theme,
    summary: String,
}

#[derive(Debug)]
struct ClipboardUpdate {
    message: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Overview,
    Models,
    Providers,
}

impl Page {
    pub fn next(self) -> Self {
        match self {
            Self::Overview => Self::Models,
            Self::Models => Self::Providers,
            Self::Providers => Self::Overview,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Overview => Self::Providers,
            Self::Models => Self::Overview,
            Self::Providers => Self::Models,
        }
    }
}

pub struct App {
    pub data: AppData,
    pub pricing: PricingCatalog,
    pub snapshot: AnalyticsSnapshot,
    pub page: Page,
    pub range: TimeRange,
    pub theme: Theme,
    pub zero_cost_behavior: ZeroCostBehavior,
    pub should_quit: bool,
    status_message: Option<StatusMessage>,
    pub focused_model_index: usize,
    pub focused_provider_index: usize,
    pricing_updates: mpsc::UnboundedReceiver<Result<PricingCatalog>>,
    clipboard_sender: mpsc::UnboundedSender<ClipboardUpdate>,
    clipboard_updates: mpsc::UnboundedReceiver<ClipboardUpdate>,
    copy_in_progress: bool,
}

impl App {
    pub fn new(
        data: AppData,
        pricing: PricingCatalog,
        theme: Theme,
        zero_cost_behavior: ZeroCostBehavior,
    ) -> Self {
        let snapshot = build_snapshot(&data, &pricing, TimeRange::All, zero_cost_behavior);
        let (sender, receiver) = mpsc::unbounded_channel();
        let (clipboard_sender, clipboard_updates) = mpsc::unbounded_channel();
        if pricing.refresh_needed {
            let cache_path = pricing.cache_path.clone();
            tokio::spawn(refresh_remote_models(cache_path, sender));
        }

        let mut app = Self {
            data,
            pricing,
            snapshot,
            page: Page::Overview,
            range: TimeRange::All,
            theme,
            zero_cost_behavior,
            should_quit: false,
            status_message: None,
            focused_model_index: 0,
            focused_provider_index: 0,
            pricing_updates: receiver,
            clipboard_sender,
            clipboard_updates,
            copy_in_progress: false,
        };

        let notices = [
            app.data.import_stats.summary(),
            app.pricing.load_notice.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if !notices.is_empty() {
            app.set_status(notices.join(" | "));
        }

        app
    }

    pub async fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_HEIGHT),
        });

        let app_result = self.run_loop(&mut terminal).await;
        Self::restore(&mut terminal)?;
        app_result
    }

    /// 默认的 ratatui::restore 在 Inline Viewport 下有错误的行为，
    /// 此处重置终端以确保光标和输入状态正确恢复
    fn restore(terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.clear()?;
        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }

    async fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut event_stream = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(TICK_TIME_MS as _));

        while !self.should_quit {
            self.clear_expired_status();
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                Some(event) = event_stream.next() => {
                    let event = event?;
                    if let Event::Key(key) = event && key.kind == KeyEventKind::Press {
                        self.handle_key(key);
                    }
                }
                Some(pricing) = self.pricing_updates.recv() => {
                    if let Ok(pricing) = pricing {
                        self.pricing = pricing;
                        self.recompute();
                        self.set_status("Pricing cache refreshed from models.dev");
                    } else {
                        self.set_status(format!(
                            "Failed to refresh cache from models.dev; {}",
                            self.pricing.refresh_failure_hint()
                        ));
                    }
                }
                Some(update) = self.clipboard_updates.recv() => {
                    self.copy_in_progress = false;
                    self.set_status(update.message);
                }
                _ = tick.tick() => {}
            }
        }
        Ok(())
    }

    fn recompute(&mut self) {
        self.snapshot = build_snapshot(
            &self.data,
            &self.pricing,
            self.range,
            self.zero_cost_behavior,
        );
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let theme = &self.theme;
        let area = frame.area();
        let vertical = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(16),
            Constraint::Length(1),
        ]);

        let [divider, header, spacer, body, footer] =
            vertical.areas(area).map(left_aligned_content);
        let [header, body, footer] = [header, body, footer]
            .map(|area| Block::new().padding(Padding::horizontal(1)).inner(area));

        frame.render_widget(
            ratatui::widgets::Paragraph::new("─".repeat(CONTENT_WIDTH as _))
                .style(theme.muted_style()),
            divider,
        );

        self.render_header(frame, header, theme);
        frame.render_widget(ratatui::widgets::Paragraph::new(""), spacer);

        match self.page {
            Page::Overview => render_overview(frame, body, &self.snapshot, self.range, theme),
            Page::Models => render_models(
                frame,
                body,
                &self.snapshot,
                self.range,
                self.focused_model_index,
                theme,
            ),
            Page::Providers => render_providers(
                frame,
                body,
                &self.snapshot,
                self.range,
                self.focused_provider_index,
                theme,
            ),
        }

        self.render_footer(frame, footer, theme);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect, theme: &Theme) {
        let content = area;
        let line = ratatui::text::Line::from(vec![
            segment_span("Overview", self.page == Page::Overview, theme),
            segment_span("Models", self.page == Page::Models, theme),
            segment_span("Providers", self.page == Page::Providers, theme),
            ratatui::text::Span::raw("    "),
            segment_span(" All ", self.range == TimeRange::All, theme),
            segment_span("7 Days", self.range == TimeRange::Last7Days, theme),
            segment_span("30 Days", self.range == TimeRange::Last30Days, theme),
            ratatui::text::Span::raw("    "),
            ratatui::text::Span::styled(format!("{:?}", self.data.source), theme.muted_style()),
        ]);

        frame.render_widget(ratatui::widgets::Paragraph::new(line), content);
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: ratatui::layout::Rect, theme: &Theme) {
        let status = self
            .status_message
            .as_ref()
            .map(|status| status.text.as_str())
            .unwrap_or("<tab> ←/→ h/l pages | r cycle | 1/2/3 pick | <ctrl-s> share | q exit");
        frame.render_widget(
            ratatui::widgets::Paragraph::new(status).style(theme.muted_style()),
            area,
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
                self.focused_provider_index = 0;
                self.recompute();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.copy_current_page();
            }
            KeyCode::Char(value) => {
                if let Some(range) = TimeRange::from_shortcut(value) {
                    self.range = range;
                    self.focused_model_index = 0;
                    self.focused_provider_index = 0;
                    self.recompute();
                }
            }
            _ => {}
        }
    }

    fn advance_focused_model(&mut self, delta: isize) {
        match self.page {
            Page::Models => {
                if self.snapshot.models.is_empty() {
                    return;
                }

                let current = self.focused_model_index as isize;
                let total = self.snapshot.models.len() as isize;
                let next = (current + delta).rem_euclid(total) as usize;
                self.focused_model_index = next;
            }
            Page::Providers => {
                if self.snapshot.providers.is_empty() {
                    return;
                }

                let current = self.focused_provider_index as isize;
                let total = self.snapshot.providers.len() as isize;
                let next = (current + delta).rem_euclid(total) as usize;
                self.focused_provider_index = next;
            }
            Page::Overview => {}
        }
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            expires_at: Some(Instant::now() + STATUS_TTL),
        });
    }

    fn clear_expired_status(&mut self) {
        if self
            .status_message
            .as_ref()
            .and_then(|status| status.expires_at)
            .is_some_and(|expires_at| Instant::now() >= expires_at)
        {
            self.status_message = None;
        }
    }

    fn capture_current_page_buffer(&self) -> Result<ratatui::buffer::Buffer> {
        let backend = TestBackend::new(CONTENT_WIDTH, VIEWPORT_HEIGHT);
        let mut terminal = Terminal::new(backend)?;
        let frame = terminal.draw(|frame| self.render(frame))?;
        Ok(frame.buffer.clone())
    }

    fn copy_current_page(&mut self) {
        if self.copy_in_progress {
            self.set_status("Still rendering share card in background, please wait ...");
            return;
        }

        let job = match self.prepare_clipboard_job() {
            Ok(job) => job,
            Err(err) => {
                self.set_status(format!("Copy failed: {err}"));
                return;
            }
        };

        self.copy_in_progress = true;
        self.set_status("Rendering share card in background ...");

        let sender = self.clipboard_sender.clone();
        tokio::task::spawn_blocking(move || {
            let message = match copy_clipboard_job(job) {
                Ok(message) => message,
                Err(err) => format!("Copy failed: {err}"),
            };
            let _ = sender.send(ClipboardUpdate { message });
        });
    }

    fn prepare_clipboard_job(&self) -> Result<ClipboardJob> {
        Ok(ClipboardJob {
            buffer: self.capture_current_page_buffer()?,
            theme: self.theme.clone(),
            summary: self.current_page_summary(),
        })
    }

    fn current_page_summary(&self) -> String {
        match self.page {
            Page::Overview => format!(
                "oc-stats {}\nTokens: {}\nCost: {}\nSessions: {}\nMessages: {}\nPrompts: {}",
                self.range.label(),
                self.snapshot.overview.total_tokens,
                format_price_summary(&self.snapshot.overview.total_cost),
                self.snapshot.overview.sessions,
                self.snapshot.overview.messages,
                self.snapshot.overview.prompts,
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
            Page::Providers => self
                .snapshot
                .providers
                .iter()
                .take(8)
                .map(|row| {
                    format!(
                        "{}: {} tokens ({:.1}%)",
                        row.provider_id, row.total_tokens, row.percentage
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

fn copy_clipboard_job(job: ClipboardJob) -> Result<String> {
    match copy_image_to_clipboard(job.buffer, job.theme) {
        Ok(()) => Ok("Successfully copied to your clipboard".to_string()),
        Err(_) => {
            copy_text_to_clipboard(&job.summary)?;
            Ok("Image export failed, copied text summary instead".to_string())
        }
    }
}

fn copy_text_to_clipboard(summary: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(summary.to_string())?;
    Ok(())
}

fn copy_image_to_clipboard(buffer: ratatui::buffer::Buffer, theme: Theme) -> Result<()> {
    let image = render_share_card(&buffer, &theme)?;
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_image(arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Owned(image.into_raw()),
    })?;
    Ok(())
}

pub fn print_exit_art(theme_kind: ThemeKind) {
    type NumStr = [&'static str; 4];
    const O: NumStr = ["    ", "█▀▀█", "█  █", "▀▀▀▀"];
    const C: NumStr = ["    ", "█▀▀▀", "█   ", "▀▀▀▀"];
    const S: NumStr = ["    ", "▄▀▀▀", "▀▀▀█", "▀▀▀ "];
    const T: NumStr = [" ▄  ", "▀█▀▀", " █  ", "  ▀▀"];
    const A: NumStr = ["    ", "█▀▀█", "█  █", "▀▀▀ ▀"];

    fn eprint_last_line(str: &str, theme: ThemeKind) {
        for ch in str.chars() {
            match ch {
                ' ' => eprint!(
                    "{}",
                    match theme {
                        ThemeKind::Dark => "▀".bright_black(),
                        ThemeKind::Light => "▀".bright_white(),
                    }
                ),
                ch => eprint!("{}", ch),
            }
        }
    }

    fn line_2_oc(str: &str, theme: ThemeKind) -> ColoredString {
        match theme {
            ThemeKind::Dark => str.bright_black().on_black(),
            ThemeKind::Light => str.bright_black().on_bright_white(),
        }
    }

    fn line_2_stats(str: &str, theme: ThemeKind) -> ColoredString {
        match theme {
            ThemeKind::Dark => str.on_bright_black(),
            ThemeKind::Light => str.on_bright_white(),
        }
    }

    for (idx, (o, c, s, t, a)) in itertools::izip!(O, C, S, T, A).enumerate() {
        match idx {
            0..2 => eprintln!(
                "  {o} {c}  {s} {t} {a} {t} {s}",
                o = o.bright_black(),
                c = c.bright_black(),
            ),
            2 => eprintln!(
                "  {o} {c}  {s} {t} {a} {t} {s}",
                o = line_2_oc(o, theme_kind),
                c = line_2_oc(c, theme_kind),
                s = line_2_stats(s, theme_kind),
                t = line_2_stats(t, theme_kind),
                a = line_2_stats(a, theme_kind),
            ),
            3 => {
                eprint!("  {o} {c}  ", o = o.bright_black(), c = c.bright_black());
                eprint_last_line(s, theme_kind);
                eprint!(" ");
                eprint_last_line(t, theme_kind);
                eprint!(" ");
                eprint_last_line(a, theme_kind);
                eprint_last_line(t, theme_kind);
                eprint!(" ");
                eprint_last_line(s, theme_kind);
                eprintln!("");
            }
            _ => unreachable!(),
        }
    }
}
