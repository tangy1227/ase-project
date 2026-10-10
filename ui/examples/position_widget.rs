//! Standalone runner for the position widget: `cargo run -p spatializer-ui --release --example position_widget`
//!
//! Extra flags: `--svg out.svg [--light] [--size WxH]` renders one frame headlessly to SVG and exits.

use std::time::Duration;

use eframe::egui::{self, CentralPanel, Frame, Panel, RichText, TextStyle, Ui};
use spatializer_ui::{ElevationStrip, PosParam, PositionHost, PositionPad, SourceView, ThemeMode, Tokens};

/// Stand-in for the plugin's `ParamSetter`: floats plus a gesture log so automation begin/end
/// pairs can be checked by eye.
struct DemoHost {
    az: f32,
    el: f32,
    dist: f32,
    seq: u32,
    log: Vec<String>,
}

impl DemoHost {
    fn new() -> Self {
        Self { az: 30.0, el: 10.0, dist: 1.0, seq: 0, log: Vec::new() }
    }
    fn push(&mut self, s: String) {
        self.seq += 1;
        self.log.push(format!("{:>4}  {s}", self.seq));
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }
}

impl PositionHost for DemoHost {
    fn azimuth(&self) -> f32 {
        self.az
    }
    fn elevation(&self) -> f32 {
        self.el
    }
    fn distance(&self) -> f32 {
        self.dist
    }
    fn set(&mut self, param: PosParam, value: f32) {
        match param {
            PosParam::Azimuth => self.az = value,
            PosParam::Elevation => self.el = value,
            PosParam::Distance => self.dist = value,
        }
    }
    fn begin_gesture(&mut self, param: PosParam) {
        self.push(format!("begin {param:?}"));
    }
    fn end_gesture(&mut self, param: PosParam) {
        self.push(format!("end   {param:?}"));
    }
}

struct App {
    host: DemoHost,
    mode: ThemeMode,
    distance: bool,
    head_tracking: bool,
    yaw: f32,
    animate: bool,
    t: f32,
    others: Vec<SourceView>,
}

impl App {
    fn new(mode: ThemeMode) -> Self {
        Self {
            host: DemoHost::new(),
            mode,
            distance: true,
            head_tracking: false,
            yaw: 0.0,
            animate: false,
            t: 0.0,
            others: vec![
                SourceView { label: "2".into(), azimuth: -60.0, elevation: 0.0, distance: 1.8, level: 0.55, clipping: false, color_index: 1 },
                SourceView { label: "3".into(), azimuth: 150.0, elevation: -20.0, distance: 2.6, level: 0.2, clipping: false, color_index: 2 },
                SourceView { label: "4".into(), azimuth: 95.0, elevation: 35.0, distance: 0.7, level: 0.95, clipping: true, color_index: 3 },
            ],
        }
    }

    fn set_mode(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.mode = mode;
        Tokens::for_mode(mode).install(ctx);
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Spatializer").text_style(TextStyle::Heading));
            ui.label(RichText::new("position widget").weak());
            ui.separator();
            let dark = self.mode == ThemeMode::Dark;
            if ui.selectable_label(dark, "Dark").clicked() && !dark {
                self.set_mode(ui.ctx(), ThemeMode::Dark);
            }
            if ui.selectable_label(!dark, "Light").clicked() && dark {
                self.set_mode(ui.ctx(), ThemeMode::Light);
            }
            ui.separator();
            ui.checkbox(&mut self.distance, "Distance");
            ui.checkbox(&mut self.head_tracking, "Head tracking");
            if self.head_tracking {
                ui.add(egui::Slider::new(&mut self.yaw, -180.0..=180.0).suffix("°").text("yaw"));
            }
            ui.checkbox(&mut self.animate, "Animate meters");
        });
    }

    fn log_panel(&self, ui: &mut Ui) {
        ui.label(RichText::new("automation gestures (newest last)").weak());
        let n = self.log_lines();
        for line in self.host.log.iter().rev().take(n).collect::<Vec<_>>().into_iter().rev() {
            ui.label(RichText::new(line).text_style(TextStyle::Name("readout".into())));
        }
    }

    fn log_lines(&self) -> usize {
        4
    }

    fn body(&mut self, ui: &mut Ui) {
        let tokens = Tokens::get(ui.ctx());
        let level = if self.animate { 0.5 + 0.45 * (self.t * 2.1).sin() } else { 0.62 };
        let others = if self.animate {
            self.others
                .iter()
                .enumerate()
                .map(|(i, o)| SourceView { level: (0.5 + 0.5 * (self.t * (1.3 + i as f32 * 0.7)).sin()).clamp(0.0, 1.0), ..o.clone() })
                .collect::<Vec<_>>()
        } else {
            self.others.clone()
        };
        let avail = ui.available_size();
        let strip_w = ElevationStrip::width(&tokens);
        let readout_h = ElevationStrip::pad_readout_height(&tokens);
        let side = (avail.x - strip_w - tokens.space(1.0)).min(avail.y - readout_h).max(tokens.space(12.0));
        ui.horizontal(|ui| {
            let pad = PositionPad::new("pad", &mut self.host)
                .others(&others)
                .label("1")
                .level(level)
                .clipping(level > 0.9)
                .head_yaw(self.head_tracking.then_some(self.yaw))
                .distance_enabled(self.distance)
                .size(side)
                .show(ui);
            if let Some(i) = pad.selected_other {
                // Selection is the caller's business; here we just report it.
                self.host.push(format!("select source {}", others[i].label));
            }
            ElevationStrip::new("elev", &mut self.host).others(&others).height(side + readout_h).show(ui);
        });
    }
}

impl App {
    /// The whole window; shared by the eframe loop and the headless SVG render.
    fn draw(&mut self, ui: &mut Ui) {
        if self.animate {
            self.t += 1.0 / 30.0;
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        let tokens = Tokens::get(ui.ctx());
        let bg = tokens.palette.bg;
        Panel::top("bar").frame(Frame::NONE.fill(bg).inner_margin(tokens.space(1.0) as i8)).show(ui, |ui| self.top_bar(ui));
        Panel::bottom("log").frame(Frame::NONE.fill(bg).inner_margin(tokens.space(1.0) as i8)).show(ui, |ui| self.log_panel(ui));
        CentralPanel::default().frame(Frame::NONE.fill(bg).inner_margin(tokens.space(1.0) as i8)).show(ui, |ui| self.body(ui));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let light = args.iter().any(|a| a == "--light");
    let mode = if light { ThemeMode::Light } else { ThemeMode::Dark };
    if let Some(i) = args.iter().position(|a| a == "--svg") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "position_widget.svg".to_string());
        let size = args
            .iter()
            .position(|a| a == "--size")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
            .unwrap_or((560.0, 440.0));
        svg::render(mode, size, &out);
        println!("wrote {out}");
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spatializer — position widget")
            .with_inner_size([560.0, 440.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "spatializer-position-widget",
        options,
        Box::new(move |cc| {
            Tokens::for_mode(mode).install(&cc.egui_ctx);
            Ok(Box::new(App::new(mode)))
        }),
    )
}

/// Headless single-frame render to SVG, so the widget can be looked at without a window
/// (and without screen-recording permissions). Covers the shape kinds the widgets emit.
mod svg {
    use super::*;
    use eframe::egui::epaint::{ClippedShape, Shape};
    use std::fmt::Write as _;

    fn color(c: egui::Color32) -> String {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        if a == 255 {
            format!("rgb({r},{g},{b})")
        } else {
            format!("rgba({r},{g},{b},{:.3})", a as f32 / 255.0)
        }
    }

    fn stroke_attrs(s: &egui::Stroke) -> String {
        if s.width <= 0.0 || s.color == egui::Color32::TRANSPARENT {
            "stroke=\"none\"".to_string()
        } else {
            format!("stroke=\"{}\" stroke-width=\"{:.2}\"", color(s.color), s.width)
        }
    }

    fn emit(out: &mut String, shape: &Shape, clip: egui::Rect) {
        match shape {
            Shape::Noop | Shape::Callback(_) => {}
            Shape::Vec(v) => v.iter().for_each(|s| emit(out, s, clip)),
            Shape::Circle(c) => {
                let _ = writeln!(out, "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\" {}/>", c.center.x, c.center.y, c.radius, color(c.fill), stroke_attrs(&c.stroke));
            }
            Shape::LineSegment { points, stroke } => {
                let _ = writeln!(out, "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" {} stroke-linecap=\"round\"/>", points[0].x, points[0].y, points[1].x, points[1].y, stroke_attrs(stroke));
            }
            Shape::Path(p) => {
                let pts: Vec<String> = p.points.iter().map(|q| format!("{:.2},{:.2}", q.x, q.y)).collect();
                let tag = if p.closed { "polygon" } else { "polyline" };
                let stroke_color = match &p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => *c,
                    egui::epaint::ColorMode::UV(_) => egui::Color32::WHITE,
                };
                let stroke = egui::Stroke::new(p.stroke.width, stroke_color);
                let _ = writeln!(out, "<{tag} points=\"{}\" fill=\"{}\" {} stroke-linejoin=\"round\" stroke-linecap=\"round\"/>", pts.join(" "), color(p.fill), stroke_attrs(&stroke));
            }
            Shape::Rect(r) => {
                let rr = r.rect;
                let cr = r.corner_radius.nw as f32;
                let _ = writeln!(out, "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"{cr}\" fill=\"{}\" {}/>", rr.min.x, rr.min.y, rr.width(), rr.height(), color(r.fill), stroke_attrs(&r.stroke));
            }
            Shape::Text(t) => {
                let g = &t.galley;
                for row in &g.rows {
                    let text: String = row.glyphs.iter().map(|gl| gl.chr).collect();
                    if text.trim().is_empty() {
                        continue;
                    }
                    let section = g.job.sections.first(); // widgets emit single-section text
                    let font_size = section.map(|s| s.format.font_id.size).unwrap_or(12.0);
                    let family = match section.map(|s| &s.format.font_id.family) {
                        Some(egui::FontFamily::Monospace) => "ui-monospace, Menlo, monospace",
                        _ => "-apple-system, Inter, Helvetica, Arial, sans-serif",
                    };
                    let color = t.override_text_color.unwrap_or_else(|| section.map(|s| s.format.color).unwrap_or(egui::Color32::WHITE));
                    let x = t.pos.x + row.pos.x + row.glyphs.first().map(|gl| gl.pos.x).unwrap_or(0.0);
                    let baseline = t.pos.y + row.pos.y + row.size.y * 0.78;
                    let esc = text.replace('&', "&amp;").replace('<', "&lt;");
                    let _ = writeln!(out, "<text x=\"{x:.2}\" y=\"{baseline:.2}\" font-size=\"{font_size:.1}\" font-family=\"{family}\" fill=\"{}\">{esc}</text>", self::color(color));
                }
            }
            other => {
                let _ = writeln!(out, "<!-- unsupported shape {:?} clip {:?} -->", std::mem::discriminant(other), clip);
            }
        }
    }

    pub fn render(mode: ThemeMode, (w, h): (f32, f32), path: &str) {
        let ctx = egui::Context::default();
        Tokens::for_mode(mode).install(&ctx);
        let mut app = App::new(mode);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))), ..Default::default() };
        // Two passes so fonts/galleys settle.
        let _ = ctx.run_ui(raw.clone(), |ui| app.draw(ui));
        let output = ctx.run_ui(raw, |ui| app.draw(ui));
        let bg = Tokens::for_mode(mode).palette.bg;
        let mut out = String::new();
        let _ = writeln!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">");
        let _ = writeln!(out, "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>", color(bg));
        for ClippedShape { clip_rect, shape } in &output.shapes {
            emit(&mut out, shape, *clip_rect);
        }
        out.push_str("</svg>\n");
        std::fs::write(path, out).expect("write svg");
    }
}
