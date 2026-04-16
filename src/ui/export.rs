use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use color_eyre::eyre::{Context, Result};
use image::imageops::{blur, overlay};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect as ImageRect;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme::Theme;

const CARD_OUTER_MARGIN_X: u32 = 34;
const CARD_OUTER_MARGIN_Y: u32 = 30;
const CARD_INSET_X: u32 = 24;
const CARD_INSET_Y: u32 = 22;
const CARD_RADIUS: u32 = 28;
const CARD_BORDER_WIDTH: u32 = 2;
const FONT_SIZE: f32 = 24.0;
const MIN_LINE_GAP: u32 = 0;
const GLYPH_X_OFFSET: i32 = -1;
const GLYPH_Y_OFFSET: i32 = 1;
const SHADOW_PADDING_X: u32 = 52;
const SHADOW_PADDING_Y: u32 = 58;
const SHADOW_BLUR_SIGMA: f32 = 25.0;
const SHADOW_ALPHA: u8 = 72;
const SHADOW_OFFSET_Y: u32 = 14;

static FONT_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/CascadiaCodeNF.ttf"
));
static FONT_ITALIC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/CascadiaCodeNFItalic.ttf"
));

pub fn render_share_card(buffer: &Buffer, theme: &Theme) -> Result<RgbaImage> {
    let fonts = Fonts::load()?;
    let metrics = Metrics::new(&fonts.regular);
    let palette = ExportPalette::from_theme(theme);
    let content_buffer = trimmed_buffer(buffer);

    let terminal_width = u32::from(content_buffer.area.width) * metrics.cell_width;
    let terminal_height = u32::from(content_buffer.area.height) * metrics.cell_height;
    let card_width = terminal_width + CARD_INSET_X * 2;
    let card_height = terminal_height + CARD_INSET_Y * 2;
    let image_width = card_width + CARD_OUTER_MARGIN_X * 2 + SHADOW_PADDING_X * 2;
    let image_height = card_height + CARD_OUTER_MARGIN_Y * 2 + SHADOW_PADDING_Y * 2;

    let mut image = RgbaImage::from_pixel(image_width, image_height, TRANSPARENT);
    let card_x = CARD_OUTER_MARGIN_X + SHADOW_PADDING_X;
    let card_y = CARD_OUTER_MARGIN_Y + SHADOW_PADDING_Y;

    draw_shadow_layers(
        &mut image,
        card_x,
        card_y,
        card_width,
        card_height,
        &palette,
    );
    draw_filled_rounded_rect(
        &mut image,
        card_x,
        card_y,
        card_width,
        card_height,
        CARD_RADIUS,
        palette.card_border,
    );
    draw_filled_rounded_rect(
        &mut image,
        card_x + CARD_BORDER_WIDTH,
        card_y + CARD_BORDER_WIDTH,
        card_width - CARD_BORDER_WIDTH * 2,
        card_height - CARD_BORDER_WIDTH * 2,
        CARD_RADIUS.saturating_sub(CARD_BORDER_WIDTH),
        palette.card_background,
    );

    let terminal_x = card_x + CARD_INSET_X;
    let terminal_y = card_y + CARD_INSET_Y;
    draw_filled_rect_mut(
        &mut image,
        ImageRect::at(terminal_x as i32, terminal_y as i32)
            .of_size(terminal_width, terminal_height),
        palette.default_background,
    );

    draw_buffer_backgrounds(
        &mut image,
        &content_buffer,
        terminal_x,
        terminal_y,
        &metrics,
        &palette,
    );
    draw_buffer_text(
        &mut image,
        &content_buffer,
        terminal_x,
        terminal_y,
        &metrics,
        &palette,
        &fonts,
    );

    Ok(image)
}

struct Fonts {
    regular: FontArc,
    italic: FontArc,
}

impl Fonts {
    fn load() -> Result<Self> {
        Ok(Self {
            regular: FontArc::try_from_slice(FONT_REGULAR)
                .wrap_err("failed to load regular export font")?,
            italic: FontArc::try_from_slice(FONT_ITALIC)
                .wrap_err("failed to load italic export font")?,
        })
    }
}

struct Metrics {
    scale: PxScale,
    cell_width: u32,
    cell_height: u32,
    glyph_x_offset: i32,
    line_top_padding: i32,
}

impl Metrics {
    fn new(font: &FontArc) -> Self {
        let scale = PxScale::from(FONT_SIZE);
        let scaled = font.as_scaled(scale);
        let line_height = (scaled.ascent() - scaled.descent()).ceil().max(1.0) as u32;
        let line_gap = scaled.line_gap().ceil().max(MIN_LINE_GAP as f32) as u32;
        let cell_height = line_height + line_gap + 4;
        let line_top_padding = ((cell_height - line_height) / 2) as i32 + GLYPH_Y_OFFSET;
        let sample_width = scaled.h_advance(font.glyph_id('0')).round().max(1.0) as u32;
        Self {
            scale,
            cell_width: sample_width,
            cell_height,
            glyph_x_offset: GLYPH_X_OFFSET,
            line_top_padding,
        }
    }
}

struct ExportPalette {
    card_background: Rgba<u8>,
    card_border: Rgba<u8>,
    shadow: Rgba<u8>,
    default_foreground: Rgba<u8>,
    default_background: Rgba<u8>,
}

impl ExportPalette {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            card_background: rgba_from_color(theme.card_background),
            card_border: rgba_from_color(theme.card_border),
            shadow: rgba_from_color(theme.card_shadow),
            default_foreground: rgba_from_color(theme.foreground),
            default_background: rgba_from_color(theme.card_background),
        }
    }
}

fn trimmed_buffer(buffer: &Buffer) -> Buffer {
    if buffer.area.height <= 3 {
        return buffer.clone();
    }

    let trimmed_height = buffer.area.height - 3;
    let trimmed_width = buffer.area.width;
    let mut content = Vec::with_capacity(trimmed_width as usize * trimmed_height as usize);

    for y in 1..buffer.area.height - 2 {
        for x in 0..trimmed_width {
            if let Some(cell) = buffer.cell((x, y)) {
                content.push(cell.clone());
            }
        }
    }

    Buffer {
        area: ratatui::layout::Rect::new(0, 0, trimmed_width, trimmed_height),
        content,
    }
}

fn draw_shadow_layers(
    image: &mut RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    palette: &ExportPalette,
) {
    let mut shadow = RgbaImage::from_pixel(image.width(), image.height(), TRANSPARENT);
    draw_filled_rounded_rect(
        &mut shadow,
        x,
        y + SHADOW_OFFSET_Y,
        width,
        height,
        CARD_RADIUS,
        with_alpha(palette.shadow, SHADOW_ALPHA),
    );

    let blurred = blur(&shadow, SHADOW_BLUR_SIGMA);
    overlay(image, &blurred, 0, 0);
}

fn draw_buffer_backgrounds(
    image: &mut RgbaImage,
    buffer: &Buffer,
    origin_x: u32,
    origin_y: u32,
    metrics: &Metrics,
    palette: &ExportPalette,
) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let Some(cell) = buffer.cell((x, y)) else {
                continue;
            };
            let bg = color_to_rgba(cell.bg, palette.default_background);
            if bg == palette.default_background {
                continue;
            }

            let px = origin_x + u32::from(x) * metrics.cell_width;
            let py = origin_y + u32::from(y) * metrics.cell_height;
            draw_filled_rect_mut(
                image,
                ImageRect::at(px as i32, py as i32)
                    .of_size(metrics.cell_width, metrics.cell_height),
                bg,
            );
        }
    }
}

fn draw_buffer_text(
    image: &mut RgbaImage,
    buffer: &Buffer,
    origin_x: u32,
    origin_y: u32,
    metrics: &Metrics,
    palette: &ExportPalette,
    fonts: &Fonts,
) {
    for y in 0..buffer.area.height {
        let mut skip = 0usize;
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }

            let Some(cell) = buffer.cell((x, y)) else {
                continue;
            };
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }

            let cell_span = UnicodeWidthStr::width(symbol).max(1) as u32;
            let font = if cell.modifier.contains(Modifier::ITALIC) {
                &fonts.italic
            } else {
                &fonts.regular
            };
            let fg = color_to_rgba(cell.fg, palette.default_foreground);
            let px = origin_x + u32::from(x) * metrics.cell_width;
            let py = origin_y + u32::from(y) * metrics.cell_height;
            let draw_x = px as i32 + metrics.glyph_x_offset;
            let draw_y = py as i32 + metrics.line_top_padding;

            draw_text_mut(image, fg, draw_x, draw_y, metrics.scale, font, symbol);
            if cell.modifier.contains(Modifier::BOLD) {
                draw_text_mut(image, fg, draw_x + 1, draw_y, metrics.scale, font, symbol);
            }

            skip = cell_span.saturating_sub(1) as usize;
        }
    }
}

fn draw_filled_rounded_rect(
    image: &mut RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
    color: Rgba<u8>,
) {
    let radius = radius.min(width / 2).min(height / 2);
    if width == 0 || height == 0 {
        return;
    }

    if radius == 0 {
        draw_filled_rect_mut(
            image,
            ImageRect::at(x as i32, y as i32).of_size(width, height),
            color,
        );
        return;
    }

    draw_filled_rect_mut(
        image,
        ImageRect::at((x + radius) as i32, y as i32).of_size(width - radius * 2, height),
        color,
    );
    draw_filled_rect_mut(
        image,
        ImageRect::at(x as i32, (y + radius) as i32).of_size(width, height - radius * 2),
        color,
    );

    let radius = radius as i32;
    let centers = [
        (x as i32 + radius, y as i32 + radius),
        (x as i32 + width as i32 - radius - 1, y as i32 + radius),
        (x as i32 + radius, y as i32 + height as i32 - radius - 1),
        (
            x as i32 + width as i32 - radius - 1,
            y as i32 + height as i32 - radius - 1,
        ),
    ];
    for (cx, cy) in centers {
        draw_filled_circle_mut(image, (cx, cy), radius, color);
    }
}

fn color_to_rgba(color: Color, default: Rgba<u8>) -> Rgba<u8> {
    match color {
        Color::Reset => default,
        Color::Black => Rgba([0, 0, 0, 255]),
        Color::Red => Rgba([205, 49, 49, 255]),
        Color::Green => Rgba([13, 188, 121, 255]),
        Color::Yellow => Rgba([229, 229, 16, 255]),
        Color::Blue => Rgba([36, 114, 200, 255]),
        Color::Magenta => Rgba([188, 63, 188, 255]),
        Color::Cyan => Rgba([17, 168, 205, 255]),
        Color::Gray => Rgba([204, 204, 204, 255]),
        Color::DarkGray => Rgba([118, 118, 118, 255]),
        Color::LightRed => Rgba([241, 76, 76, 255]),
        Color::LightGreen => Rgba([35, 209, 139, 255]),
        Color::LightYellow => Rgba([245, 245, 67, 255]),
        Color::LightBlue => Rgba([59, 142, 234, 255]),
        Color::LightMagenta => Rgba([214, 112, 214, 255]),
        Color::LightCyan => Rgba([41, 184, 219, 255]),
        Color::White => Rgba([255, 255, 255, 255]),
        Color::Rgb(r, g, b) => Rgba([r, g, b, 255]),
        Color::Indexed(index) => xterm_index_to_rgba(index),
    }
}

const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);

fn rgba_from_color(color: Color) -> Rgba<u8> {
    color_to_rgba(color, Rgba([0, 0, 0, 255]))
}

fn with_alpha(color: Rgba<u8>, alpha: u8) -> Rgba<u8> {
    Rgba([color[0], color[1], color[2], alpha])
}

fn xterm_index_to_rgba(index: u8) -> Rgba<u8> {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    if index < 16 {
        let [r, g, b] = ANSI[index as usize];
        return Rgba([r, g, b, 255]);
    }

    if index >= 232 {
        let gray = 8 + (index - 232) * 10;
        return Rgba([gray, gray, gray, 255]);
    }

    let palette_index = index - 16;
    let r = palette_index / 36;
    let g = (palette_index % 36) / 6;
    let b = palette_index % 6;
    let component = |value: u8| if value == 0 { 0 } else { value * 40 + 55 };
    Rgba([component(r), component(g), component(b), 255])
}
