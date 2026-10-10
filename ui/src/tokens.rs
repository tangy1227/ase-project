//! Design tokens for the spatializer UI, expressed through egui's styling system.
//!
//! Widgets never hard-code colours, sizes or strokes. They read `egui::Visuals`/`egui::Style`
//! (which [`Tokens::install`] populates) and, for the few things egui has no slot for (categorical
//! source colours, stroke weights, the spacing unit), the [`Tokens`] value stored in the egui
//! context by the same call.

use egui::{
    style::{HandleShape, Selection, WidgetVisuals, Widgets},
    Color32, CornerRadius, CursorIcon, FontFamily, FontId, Id, Margin, Stroke, TextStyle, Visuals,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Colour roles. Every colour in the UI is one of these.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Window / panel background.
    pub bg: Color32,
    /// Raised surfaces: the pad disc, the elevation track, cards.
    pub surface: Color32,
    /// Lines on surfaces: rings, ticks, separators.
    pub surface_alt: Color32,
    /// Primary text and strokes.
    pub ink: Color32,
    /// Secondary text, tick labels, inactive markers.
    pub ink_muted: Color32,
    /// The active/selected source and interactive highlights.
    pub accent: Color32,
    /// Translucent accent for leader lines, arcs, hover rings.
    pub accent_soft: Color32,
    /// Warnings (near the edge of the rendered space).
    pub warn: Color32,
    /// Clipping and errors.
    pub danger: Color32,
    /// Categorical colours for up to six sources (index 0 == accent).
    pub sources: [Color32; 6],
}

#[derive(Clone, Copy, Debug)]
pub struct Radii {
    pub sm: u8,
    pub md: u8,
    pub lg: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Strokes {
    pub hairline: f32,
    pub thin: f32,
    pub regular: f32,
    pub bold: f32,
}

/// Font sizes in points.
#[derive(Clone, Copy, Debug)]
pub struct TypeScale {
    pub caption: f32,
    pub label: f32,
    pub body: f32,
    pub title: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub mode: ThemeMode,
    pub palette: Palette,
    pub radii: Radii,
    pub strokes: Strokes,
    /// Base spacing unit in points; all paddings and gaps are multiples of it.
    pub space: f32,
    pub type_scale: TypeScale,
}

impl Tokens {
    const ID: &'static str = "spatializer-ui/tokens";

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            palette: Palette {
                bg: Color32::from_rgb(0x14, 0x16, 0x1a),
                surface: Color32::from_rgb(0x1c, 0x1f, 0x24),
                surface_alt: Color32::from_rgb(0x2c, 0x31, 0x39),
                ink: Color32::from_rgb(0xe8, 0xea, 0xed),
                ink_muted: Color32::from_rgb(0x8a, 0x90, 0x99),
                accent: Color32::from_rgb(0xff, 0xb4, 0x54),
                accent_soft: Color32::from_rgba_unmultiplied(0xff, 0xb4, 0x54, 0x60),
                warn: Color32::from_rgb(0xf5, 0xc4, 0x51),
                danger: Color32::from_rgb(0xff, 0x5c, 0x5c),
                sources: [
                    Color32::from_rgb(0xff, 0xb4, 0x54),
                    Color32::from_rgb(0x4f, 0xc3, 0xf7),
                    Color32::from_rgb(0xa0, 0xe8, 0x6f),
                    Color32::from_rgb(0xf7, 0x8f, 0xb3),
                    Color32::from_rgb(0xc7, 0x9b, 0xff),
                    Color32::from_rgb(0x7f, 0xe3, 0xd0),
                ],
            },
            radii: Radii { sm: 3, md: 6, lg: 10 },
            strokes: Strokes { hairline: 1.0, thin: 1.5, regular: 2.0, bold: 3.0 },
            space: 8.0,
            type_scale: TypeScale { caption: 11.0, label: 12.0, body: 13.0, title: 16.0 },
        }
    }

    pub fn light() -> Self {
        let mut t = Self::dark();
        t.mode = ThemeMode::Light;
        t.palette = Palette {
            bg: Color32::from_rgb(0xf4, 0xf5, 0xf7),
            surface: Color32::from_rgb(0xff, 0xff, 0xff),
            surface_alt: Color32::from_rgb(0xd9, 0xdd, 0xe3),
            ink: Color32::from_rgb(0x1b, 0x1f, 0x24),
            ink_muted: Color32::from_rgb(0x6b, 0x72, 0x80),
            accent: Color32::from_rgb(0xd9, 0x77, 0x06),
            accent_soft: Color32::from_rgba_unmultiplied(0xd9, 0x77, 0x06, 0x60),
            warn: Color32::from_rgb(0xb4, 0x83, 0x00),
            danger: Color32::from_rgb(0xd3, 0x2f, 0x2f),
            sources: [
                Color32::from_rgb(0xd9, 0x77, 0x06),
                Color32::from_rgb(0x02, 0x77, 0xbd),
                Color32::from_rgb(0x4c, 0x8c, 0x1e),
                Color32::from_rgb(0xc2, 0x18, 0x5b),
                Color32::from_rgb(0x6a, 0x3f, 0xc7),
                Color32::from_rgb(0x00, 0x83, 0x7a),
            ],
        };
        t
    }

    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    /// `n` spacing units in points.
    pub fn space(&self, n: f32) -> f32 {
        self.space * n
    }

    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// Colour for source `index` (wraps around).
    pub fn source_color(&self, index: usize) -> Color32 {
        self.palette.sources[index % self.palette.sources.len()]
    }

    /// Push the tokens into egui: theme, `Visuals`, `Style` (spacing + type scale) and the context
    /// data store so widgets can read the non-egui parts back with [`Tokens::get`].
    pub fn install(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.mode {
            ThemeMode::Dark => egui::Theme::Dark,
            ThemeMode::Light => egui::Theme::Light,
        });
        let visuals = self.visuals();
        ctx.set_visuals(visuals.clone());
        ctx.global_style_mut(|style| {
            style.visuals = visuals.clone();
            self.apply_spacing(&mut style.spacing);
            style.text_styles = self.text_styles().into_iter().collect();
        });
        ctx.data_mut(|d| d.insert_temp(Id::new(Self::ID), *self));
    }

    /// Read the installed tokens back (dark defaults if `install` was never called).
    pub fn get(ctx: &egui::Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(Id::new(Self::ID)))
            .unwrap_or_else(Tokens::dark)
    }

    pub fn text_styles(&self) -> Vec<(TextStyle, FontId)> {
        let ts = self.type_scale;
        vec![
            (TextStyle::Small, self.font(ts.caption)),
            (TextStyle::Body, self.font(ts.body)),
            (TextStyle::Button, self.font(ts.label)),
            (TextStyle::Heading, self.font(ts.title)),
            (TextStyle::Monospace, self.mono(ts.body)),
            (TextStyle::Name("readout".into()), self.mono(ts.caption)),
        ]
    }

    fn apply_spacing(&self, spacing: &mut egui::style::Spacing) {
        let s = self.space;
        spacing.item_spacing = egui::vec2(s, s);
        spacing.button_padding = egui::vec2(s * 1.5, s * 0.75);
        spacing.interact_size = egui::vec2(s * 5.0, s * 3.0);
        spacing.window_margin = Margin::same(s as i8);
        spacing.menu_margin = Margin::same(s as i8);
        spacing.slider_width = s * 16.0;
        spacing.indent = s * 2.0;
    }

    pub fn visuals(&self) -> Visuals {
        let p = self.palette;
        let st = self.strokes;
        let mut v = match self.mode {
            ThemeMode::Dark => Visuals::dark(),
            ThemeMode::Light => Visuals::light(),
        };
        v.dark_mode = self.mode == ThemeMode::Dark;
        v.override_text_color = Some(p.ink);
        v.weak_text_color = Some(p.ink_muted);
        v.panel_fill = p.bg;
        v.window_fill = p.bg;
        v.window_stroke = Stroke::new(st.hairline, p.surface_alt);
        v.extreme_bg_color = p.surface;
        v.faint_bg_color = p.surface_alt;
        v.code_bg_color = p.surface;
        v.hyperlink_color = p.accent;
        v.warn_fg_color = p.warn;
        v.error_fg_color = p.danger;
        v.window_corner_radius = CornerRadius::same(self.radii.lg);
        v.menu_corner_radius = CornerRadius::same(self.radii.md);
        v.slider_trailing_fill = true;
        v.handle_shape = HandleShape::Circle;
        v.interact_cursor = Some(CursorIcon::PointingHand);
        v.selection = Selection {
            bg_fill: p.accent,
            stroke: Stroke::new(st.regular, p.bg),
        };
        let wv = |bg: Color32, weak: Color32, bg_stroke: Stroke, fg: Stroke, r: u8, expansion: f32| {
            WidgetVisuals {
                bg_fill: bg,
                weak_bg_fill: weak,
                bg_stroke,
                corner_radius: CornerRadius::same(r),
                fg_stroke: fg,
                expansion,
            }
        };
        v.widgets = Widgets {
            noninteractive: wv(
                p.surface,
                p.surface,
                Stroke::new(st.hairline, p.surface_alt),
                Stroke::new(st.regular, p.ink_muted),
                self.radii.sm,
                0.0,
            ),
            inactive: wv(
                p.surface,
                p.surface,
                Stroke::new(st.thin, p.surface_alt),
                Stroke::new(st.regular, p.ink),
                self.radii.md,
                0.0,
            ),
            hovered: wv(
                p.surface_alt,
                p.surface_alt,
                Stroke::new(st.thin, p.accent_soft),
                Stroke::new(st.bold, p.ink),
                self.radii.md,
                1.0,
            ),
            active: wv(
                p.accent_soft,
                p.accent_soft,
                Stroke::new(st.regular, p.accent),
                Stroke::new(st.bold, p.ink),
                self.radii.md,
                1.0,
            ),
            open: wv(
                p.surface_alt,
                p.surface_alt,
                Stroke::new(st.thin, p.accent_soft),
                Stroke::new(st.regular, p.ink),
                self.radii.md,
                0.0,
            ),
        };
        v
    }
}
