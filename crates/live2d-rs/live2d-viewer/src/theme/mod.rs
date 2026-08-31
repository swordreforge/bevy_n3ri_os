//! # Theme — Decoupled theming system for live2d-viewer
//!
//! Inspired by [@proj-airi/ui](https://github.com/moeru-ai/airi): chromatic color
//! generation, glassmorphism aesthetic, dark/light mode, button/callout variants.
//!
//! ## Usage
//!
//! ```no_run
//! let theme = Theme::aira().dark();
//! theme::apply_theme(&ctx, &theme);
//!
//! // Styled widgets — same API as egui, with AIRI-inspired variants
//! if theme::button(ui, theme::ButtonVariant::Primary, "Save").clicked() { }
//! theme::callout_frame(ui, theme::CalloutVariant::Primary, "Info", |ui| { ui.label("…"); });
//! ```

// Most items are public API for future use even if not consumed yet.
#![allow(dead_code)]

use egui::{Color32, FontId, Margin, Rounding, Stroke, Style, Vec2};

// ---------------------------------------------------------------------------
// Palette — chromatic hue → 10-stop color scale
// ---------------------------------------------------------------------------

/// A 10-stop palette (50 … 950) matching UnoCSS naming.
#[derive(Clone, Debug)]
pub struct Palette {
    pub c50: Color32,
    pub c100: Color32,
    pub c200: Color32,
    pub c300: Color32,
    pub c400: Color32,
    pub c500: Color32,
    pub c600: Color32,
    pub c700: Color32,
    pub c800: Color32,
    pub c900: Color32,
    pub c950: Color32,
}

/// HSL → sRGB `Color32`.
fn hsl(hue: f32, sat: f32, light: f32) -> Color32 {
    let c = (1.0f32 - (2.0f32 * light - 1.0f32).abs()) * sat;
    let x = c * (1.0f32 - ((hue / 60.0f32) % 2.0f32 - 1.0f32).abs());
    let m = light - c / 2.0f32;
    let (r, g, b) = match hue as i32 {
        0..=59 => (c, x, 0.0f32),
        60..=119 => (x, c, 0.0f32),
        120..=179 => (0.0f32, c, x),
        180..=239 => (0.0f32, x, c),
        240..=299 => (x, 0.0f32, c),
        _ => (c, 0.0f32, x),
    };
    Color32::from_rgb(
        ((r + m) * 255.0f32) as u8,
        ((g + m) * 255.0f32) as u8,
        ((b + m) * 255.0f32) as u8,
    )
}

/// Build a 10-stop palette from hue (0–360) and chroma factor.
fn make_palette(hue: f32, chroma: f32) -> Palette {
    let lightness = |stop: u16| match stop {
        50 => 0.95f32,
        100 => 0.90f32,
        200 => 0.80f32,
        300 => 0.70f32,
        400 => 0.60f32,
        500 => 0.50f32,
        600 => 0.40f32,
        700 => 0.30f32,
        800 => 0.20f32,
        900 => 0.12f32,
        950 => 0.06f32,
        _ => 0.50f32,
    };

    let saturation = |stop: u16| match stop {
        50 => chroma * 0.30f32,
        100 => chroma * 0.50f32,
        200 => chroma * 0.60f32,
        300 => chroma * 0.70f32,
        400 => chroma * 0.80f32,
        500 => chroma,
        600 => chroma * 0.90f32,
        700 => chroma * 0.85f32,
        800 => chroma * 0.75f32,
        900 => chroma * 0.65f32,
        950 => chroma * 0.50f32,
        _ => chroma,
    };

    let map = |stop| hsl(hue, saturation(stop), lightness(stop));
    Palette {
        c50: map(50),
        c100: map(100),
        c200: map(200),
        c300: map(300),
        c400: map(400),
        c500: map(500),
        c600: map(600),
        c700: map(700),
        c800: map(800),
        c900: map(900),
        c950: map(950),
    }
}

// ---------------------------------------------------------------------------
// Neutral palette (greys)
// ---------------------------------------------------------------------------

fn neutral_palette(is_dark: bool) -> Palette {
    if is_dark {
        Palette {
            c50: Color32::from_rgb(0x17, 0x17, 0x1a),
            c100: Color32::from_rgb(0x1e, 0x1e, 0x22),
            c200: Color32::from_rgb(0x2a, 0x2a, 0x2e),
            c300: Color32::from_rgb(0x38, 0x38, 0x3c),
            c400: Color32::from_rgb(0x4a, 0x4a, 0x4e),
            c500: Color32::from_rgb(0x65, 0x65, 0x69),
            c600: Color32::from_rgb(0x82, 0x82, 0x86),
            c700: Color32::from_rgb(0xa0, 0xa0, 0xa4),
            c800: Color32::from_rgb(0xbf, 0xbf, 0xc2),
            c900: Color32::from_rgb(0xdf, 0xdf, 0xe1),
            c950: Color32::from_rgb(0xf0, 0xf0, 0xf2),
        }
    } else {
        Palette {
            c50: Color32::from_rgb(0xf8, 0xf8, 0xfa),
            c100: Color32::from_rgb(0xf0, 0xf0, 0xf4),
            c200: Color32::from_rgb(0xe5, 0xe5, 0xea),
            c300: Color32::from_rgb(0xd4, 0xd4, 0xd9),
            c400: Color32::from_rgb(0xa0, 0xa0, 0xa8),
            c500: Color32::from_rgb(0x7a, 0x7a, 0x82),
            c600: Color32::from_rgb(0x60, 0x60, 0x66),
            c700: Color32::from_rgb(0x45, 0x45, 0x4a),
            c800: Color32::from_rgb(0x2e, 0x2e, 0x33),
            c900: Color32::from_rgb(0x1a, 0x1a, 0x1e),
            c950: Color32::from_rgb(0x0f, 0x0f, 0x12),
        }
    }
}

// ---------------------------------------------------------------------------
// Theme — the top-level config
// ---------------------------------------------------------------------------

/// A visual theme for the viewer.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub dark_mode: bool,
    pub primary: Palette,
    pub neutral: Palette,
    pub danger: Palette,
    pub caution: Palette,
    pub success: Palette,
}

impl Theme {
    /// AIRI default (hue ≈ 220, blue-lavender).
    pub fn aira() -> ThemeBuilder {
        ThemeBuilder {
            name: "AIRA",
            hue: 220.44,
            chroma: 0.55,
        }
    }

    /// Warm coral (hue ≈ 10).
    pub fn coral() -> ThemeBuilder {
        ThemeBuilder {
            name: "Coral",
            hue: 10.0,
            chroma: 0.50,
        }
    }

    /// Fresh mint (hue ≈ 160).
    pub fn mint() -> ThemeBuilder {
        ThemeBuilder {
            name: "Mint",
            hue: 160.0,
            chroma: 0.40,
        }
    }

    /// Vibrant purple (hue ≈ 270).
    pub fn purple() -> ThemeBuilder {
        ThemeBuilder {
            name: "Purple",
            hue: 270.0,
            chroma: 0.45,
        }
    }

    /// Convert the theme into an egui `Style`.
    pub fn to_style(&self) -> Style {
        let p = &self.primary;
        let n = &self.neutral;
        let d = &self.danger;
        let c = &self.caution;

        let text_color = n.c900;

        Style {
            visuals: egui::Visuals {
                dark_mode: self.dark_mode,
                override_text_color: Some(text_color),
                window_fill: if self.dark_mode { n.c100 } else { n.c100 },
                window_stroke: Stroke::new(1.0_f32, if self.dark_mode { n.c400 } else { n.c300 }),
                window_rounding: Rounding::same(10.0),
                window_shadow: egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(60),
                },
                panel_fill: if self.dark_mode { n.c100 } else { n.c100 },
                faint_bg_color: if self.dark_mode { n.c200 } else { n.c100 },
                extreme_bg_color: if self.dark_mode { n.c50 } else { n.c50 },
                code_bg_color: if self.dark_mode { n.c200 } else { n.c200 },
                warn_fg_color: c.c500,
                error_fg_color: d.c500,
                hyperlink_color: p.c500,
                selection: egui::style::Selection {
                    bg_fill: p.c500.linear_multiply(0.3),
                    stroke: Stroke::new(1.0_f32, p.c500),
                },
                widgets: egui::style::Widgets {
                    noninteractive: egui::style::WidgetVisuals {
                        bg_fill: if self.dark_mode { n.c100 } else { n.c100 },
                        weak_bg_fill: if self.dark_mode { n.c200 } else { n.c100 },
                        bg_stroke: Stroke::new(
                            1.0_f32,
                            if self.dark_mode { n.c400 } else { n.c300 },
                        ),
                        rounding: Rounding::same(8.0),
                        fg_stroke: Stroke::new(1.0_f32, text_color),
                        expansion: 0.0,
                    },
                    inactive: egui::style::WidgetVisuals {
                        bg_fill: if self.dark_mode { n.c200 } else { n.c100 },
                        weak_bg_fill: if self.dark_mode { n.c300 } else { n.c200 },
                        bg_stroke: Stroke::new(
                            1.0_f32,
                            if self.dark_mode { n.c400 } else { n.c300 },
                        ),
                        rounding: Rounding::same(8.0),
                        fg_stroke: Stroke::new(1.0_f32, text_color),
                        expansion: 0.0,
                    },
                    hovered: egui::style::WidgetVisuals {
                        bg_fill: p.c500.linear_multiply(0.15),
                        weak_bg_fill: p.c500.linear_multiply(0.10),
                        bg_stroke: Stroke::new(1.5_f32, p.c500.linear_multiply(0.4)),
                        rounding: Rounding::same(8.0),
                        fg_stroke: Stroke::new(2.0_f32, p.c500),
                        expansion: 2.0,
                    },
                    active: egui::style::WidgetVisuals {
                        bg_fill: p.c500.linear_multiply(0.30),
                        weak_bg_fill: p.c500.linear_multiply(0.20),
                        bg_stroke: Stroke::new(2.0_f32, p.c600),
                        rounding: Rounding::same(7.0),
                        fg_stroke: Stroke::new(2.5_f32, p.c600),
                        expansion: -0.5,
                    },
                    open: egui::style::WidgetVisuals {
                        bg_fill: p.c500.linear_multiply(0.10),
                        weak_bg_fill: p.c500.linear_multiply(0.05),
                        bg_stroke: Stroke::new(1.0_f32, p.c500.linear_multiply(0.2)),
                        rounding: Rounding::same(8.0),
                        fg_stroke: Stroke::new(1.5_f32, p.c500),
                        expansion: 0.0,
                    },
                },
                menu_rounding: Rounding::same(10.0),
                ..Default::default()
            },
            spacing: egui::style::Spacing {
                item_spacing: Vec2::new(8.0, 6.0),
                button_padding: Vec2::new(14.0, 5.0),
                indent: 16.0,
                window_margin: Margin::symmetric(8.0, 6.0),
                menu_margin: Margin::symmetric(4.0, 2.0),
                interact_size: Vec2::new(40.0, 24.0),
                ..Default::default()
            },
            text_styles: [
                (
                    egui::TextStyle::Heading,
                    FontId::new(18.0, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Body,
                    FontId::new(14.0, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Monospace,
                    FontId::new(13.0, egui::FontFamily::Monospace),
                ),
                (
                    egui::TextStyle::Button,
                    FontId::new(14.0, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Small,
                    FontId::new(12.0, egui::FontFamily::Proportional),
                ),
            ]
            .into(),
            animation_time: 0.15,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// ThemeBuilder
// ---------------------------------------------------------------------------

pub struct ThemeBuilder {
    name: &'static str,
    hue: f32,
    chroma: f32,
}

impl ThemeBuilder {
    pub fn dark(&self) -> Theme {
        Theme {
            name: self.name,
            dark_mode: true,
            primary: make_palette(self.hue, self.chroma),
            neutral: neutral_palette(true),
            danger: make_palette(0.0, 0.50),
            caution: make_palette(35.0, 0.50),
            success: make_palette(145.0, 0.40),
        }
    }

    pub fn light(&self) -> Theme {
        Theme {
            name: self.name,
            dark_mode: false,
            primary: make_palette(self.hue, self.chroma),
            neutral: neutral_palette(false),
            danger: make_palette(0.0, 0.50),
            caution: make_palette(35.0, 0.50),
            success: make_palette(145.0, 0.40),
        }
    }
}

// ---------------------------------------------------------------------------
// Styled Widget Helpers
// ---------------------------------------------------------------------------

/// Button variants matching `@proj-airi/ui`'s button component.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    SecondaryMuted,
    Danger,
    Caution,
    Pure,
    Ghost,
}

/// Callout variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalloutVariant {
    Primary,
    Violet,
    Lime,
    Orange,
}

/// Build a styled `Button` widget. Add it via `ui.add()` or chain `.clicked()`.
pub fn button(
    ui: &egui::Ui,
    variant: ButtonVariant,
    text: impl Into<egui::WidgetText>,
) -> egui::Button<'static> {
    let visuals = &ui.visuals();

    let (bg_fill, bg_stroke, _) = match variant {
        ButtonVariant::Primary => {
            let p = &visuals.widgets.inactive;
            (p.bg_fill, p.bg_stroke, p.fg_stroke.color)
        }
        ButtonVariant::Secondary => {
            let n = &visuals.widgets.inactive;
            (n.weak_bg_fill, n.bg_stroke, n.fg_stroke.color)
        }
        ButtonVariant::SecondaryMuted => (
            visuals.widgets.inactive.bg_fill,
            visuals.widgets.inactive.bg_stroke,
            visuals.text_color(),
        ),
        ButtonVariant::Danger => {
            let color = visuals.error_fg_color;
            (
                color.linear_multiply(0.15),
                Stroke::new(1.0_f32, color.linear_multiply(0.3)),
                color,
            )
        }
        ButtonVariant::Caution => {
            let color = visuals.warn_fg_color;
            (
                color.linear_multiply(0.15),
                Stroke::new(1.0_f32, color.linear_multiply(0.3)),
                color,
            )
        }
        ButtonVariant::Pure => (Color32::TRANSPARENT, Stroke::NONE, visuals.text_color()),
        ButtonVariant::Ghost => (Color32::TRANSPARENT, Stroke::NONE, visuals.text_color()),
    };

    let mut btn = egui::Button::new(text)
        .fill(bg_fill)
        .stroke(bg_stroke)
        .rounding(8.0);

    if variant == ButtonVariant::Pure {
        btn = btn.min_size(Vec2::ZERO);
    }

    btn
}

/// A callout box with a colored left accent bar (mirrors AIRI's `Callout`).
pub fn callout_frame(
    ui: &mut egui::Ui,
    variant: CalloutVariant,
    label: &str,
    add_body: impl FnOnce(&mut egui::Ui),
) {
    let visuals = ui.visuals();

    let (accent, bg) = match variant {
        CalloutVariant::Primary => {
            let p = visuals.hyperlink_color;
            (p, p.linear_multiply(0.08))
        }
        CalloutVariant::Violet => {
            let c = Color32::from_rgb(0x8b, 0x5c, 0xf6);
            (c, c.linear_multiply(0.08))
        }
        CalloutVariant::Lime => {
            let c = Color32::from_rgb(0x84, 0xcc, 0x16);
            (c, c.linear_multiply(0.08))
        }
        CalloutVariant::Orange => {
            let c = visuals.warn_fg_color;
            (c, c.linear_multiply(0.08))
        }
    };

    let frame = egui::Frame {
        inner_margin: Margin::symmetric(4.0, 8.0),
        outer_margin: Margin::ZERO,
        rounding: Rounding::same(6.0),
        shadow: egui::epaint::Shadow::default(),
        fill: bg,
        stroke: Stroke::NONE,
    };

    frame.show(ui, |ui| {
        // Accent bar on the left
        let bar_rect = egui::Rect::from_min_size(
            ui.max_rect().left_top(),
            egui::vec2(3.0, ui.available_height().max(0.0)),
        );
        ui.painter()
            .rect_filled(bar_rect, Rounding::same(2.0), accent);

        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).color(accent).strong());
                add_body(ui);
            });
        });
    });
}

/// A collapsible section with a clickable header.
pub fn collapsible(
    ui: &mut egui::Ui,
    label: &str,
    open: &mut bool,
    add_body: impl FnOnce(&mut egui::Ui),
) {
    let resp = ui.interact(
        egui::Rect::from_min_size(
            ui.cursor().left_top(),
            egui::vec2(ui.available_width(), 26.0),
        ),
        egui::Id::new(label),
        egui::Sense::click(),
    );

    let header_color = if resp.hovered() {
        ui.visuals().hyperlink_color
    } else {
        ui.visuals().text_color()
    };

    let arrow = if *open { "▼" } else { "▶" };
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{arrow} {label}")).color(header_color));
    });

    if resp.clicked() {
        *open = !*open;
    }

    if *open {
        ui.indent(label, |ui| {
            add_body(ui);
        });
    }
}

/// Apply the theme to an egui `Context`.
pub fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    ctx.set_style(theme.to_style());
}

// ---------------------------------------------------------------------------
// Convenience — one-liner for context
// ---------------------------------------------------------------------------

/// Shorthand: build and apply the AIRI dark theme.
pub fn apply_aira_dark(ctx: &egui::Context) {
    apply_theme(ctx, &Theme::aira().dark());
}

/// Shorthand: build and apply the AIRI light theme.
pub fn apply_aira_light(ctx: &egui::Context) {
    apply_theme(ctx, &Theme::aira().light());
}
