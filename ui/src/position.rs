//! Position widgets: a top-down pad (azimuth + distance) and an elevation strip.
//!
//! Conventions follow SOFA: azimuth 0° is in front, positive azimuth is to the listener's LEFT,
//! elevation is positive upwards. The readout prints "30° L" / "30° R" so the sign convention is
//! never exposed to the user.
//!
//! Interaction (no typed entry anywhere):
//! - drag: azimuth (and distance when enabled) follow the pointer;
//! - click: place;
//! - shift + drag: fine (1/10 speed, relative to the current value, no jump);
//! - alt/option + drag: snap to 15° (azimuth/elevation) and 0.25 m (distance);
//! - double-click or cmd/ctrl-click: reset to the parameter defaults;
//! - wheel while hovering: nudge; shift makes the nudge fine;
//! - arrow keys while hovering: 1° nudges (works standalone; hosts often eat key events).
//!
//! Every value change goes through [`PositionHost`], so the plugin adapter can emit
//! begin/end automation gestures exactly where a drag starts and stops.

use std::f32::consts::{PI, TAU};

use egui::{pos2, vec2, Align2, CornerRadius, CursorIcon, Id, Key, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::tokens::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PosParam {
    Azimuth,
    Elevation,
    Distance,
}

/// The widget's only link to the outside world. In the plugin this wraps `ParamSetter`;
/// in the standalone example it is a struct of floats plus a gesture log.
pub trait PositionHost {
    /// Degrees, −180…180, positive = left.
    fn azimuth(&self) -> f32;
    /// Degrees, −90…90, positive = up.
    fn elevation(&self) -> f32;
    /// Metres.
    fn distance(&self) -> f32;
    fn set(&mut self, param: PosParam, value: f32);
    /// Called once when a drag starts on `param` (host: `begin_set_parameter`).
    fn begin_gesture(&mut self, param: PosParam);
    /// Called once when that drag ends (host: `end_set_parameter`).
    fn end_gesture(&mut self, param: PosParam);
    fn default_value(&self, param: PosParam) -> f32 {
        match param {
            PosParam::Azimuth | PosParam::Elevation => 0.0,
            PosParam::Distance => 1.0,
        }
    }
    /// (min, max) distance in metres; the pad's outer ring is `max`.
    fn distance_range(&self) -> (f32, f32) {
        (0.2, 3.0)
    }
}

pub const SNAP_DEG: f32 = 15.0;
pub const SNAP_DIST: f32 = 0.25;
pub const FINE_SCALE: f32 = 0.1;
pub const WHEEL_DEG_PER_POINT: f32 = 0.05;
/// Where the single ring sits when distance is not a parameter.
const RING_FRACTION: f32 = 0.72;

/// Wrap to (−180, 180]; 180 is canonical for "behind".
pub fn wrap_deg(a: f32) -> f32 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w <= -180.0 {
        w + 360.0
    } else {
        w
    }
}

pub fn snap(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

/// SOFA azimuth to a unit vector in screen space (x right, y down); front is up.
pub fn az_to_dir(az_deg: f32) -> Vec2 {
    let a = az_deg.to_radians();
    vec2(-a.sin(), -a.cos())
}

/// Screen offset from the head centre to SOFA azimuth.
pub fn dir_to_az(d: Vec2) -> f32 {
    if d == Vec2::ZERO {
        0.0
    } else {
        (-d.x).atan2(-d.y).to_degrees()
    }
}

pub fn format_azimuth(az: f32) -> String {
    let a = wrap_deg(az);
    if a.abs() < 0.05 {
        "0° F".to_string()
    } else if (a.abs() - 180.0).abs() < 0.05 {
        "180° B".to_string()
    } else {
        format!("{:.0}° {}", a.abs(), if a > 0.0 { "L" } else { "R" })
    }
}

pub fn format_elevation(el: f32) -> String {
    if el.abs() < 0.05 {
        "0°".to_string()
    } else {
        format!("{}{:.0}°", if el > 0.0 { "+" } else { "−" }, el.abs())
    }
}

pub fn format_distance(d: f32) -> String {
    format!("{d:.2} m")
}

/// A source drawn for context (another object, or the same source on another track).
#[derive(Clone, Debug)]
pub struct SourceView {
    pub label: String,
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
    /// 0…1 linear level for the meter arc.
    pub level: f32,
    pub clipping: bool,
    pub color_index: usize,
}

#[derive(Clone, Copy, Default)]
struct DragState {
    last_angle: f32,
    last_radius: f32,
    az: f32,
    dist: f32,
}

#[derive(Clone, Copy, Default)]
struct StripDrag {
    last_y: f32,
    el: f32,
}

pub struct PadResponse {
    pub response: Response,
    /// A parameter was changed this frame.
    pub changed: bool,
    /// The user clicked one of the `others`; the caller decides what selection means.
    pub selected_other: Option<usize>,
}

/// Top-down view: azimuth is the angle around the head, distance the radius.
pub struct PositionPad<'a> {
    id: Id,
    host: &'a mut dyn PositionHost,
    others: &'a [SourceView],
    label: &'a str,
    level: f32,
    clipping: bool,
    head_yaw: Option<f32>,
    distance_enabled: bool,
    color_index: usize,
    size: Option<f32>,
}

impl<'a> PositionPad<'a> {
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, host: &'a mut dyn PositionHost) -> Self {
        Self {
            id: Id::new(id),
            host,
            others: &[],
            label: "1",
            level: 0.0,
            clipping: false,
            head_yaw: None,
            distance_enabled: false,
            color_index: 0,
            size: None,
        }
    }

    /// Other sources to draw as ghosts (selectable by click).
    pub fn others(mut self, others: &'a [SourceView]) -> Self {
        self.others = others;
        self
    }

    /// Short label drawn inside the active dot ("1", "L", "R"…).
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }

    /// Meter arc around the active dot, 0…1.
    pub fn level(mut self, level: f32) -> Self {
        self.level = level;
        self
    }

    pub fn clipping(mut self, clipping: bool) -> Self {
        self.clipping = clipping;
        self
    }

    /// Listener yaw in degrees (positive = turned left) when head-tracking is active.
    pub fn head_yaw(mut self, yaw: Option<f32>) -> Self {
        self.head_yaw = yaw;
        self
    }

    /// When false the dot is constrained to one ring and the radius is not a parameter.
    pub fn distance_enabled(mut self, enabled: bool) -> Self {
        self.distance_enabled = enabled;
        self
    }

    pub fn color_index(mut self, index: usize) -> Self {
        self.color_index = index;
        self
    }

    /// Side length in points; defaults to the largest square that fits.
    pub fn size(mut self, side: f32) -> Self {
        self.size = Some(side);
        self
    }

    pub fn show(self, ui: &mut Ui) -> PadResponse {
        let PositionPad { id, host, others, label, level, clipping, head_yaw, distance_enabled, color_index, size } = self;
        let tokens = Tokens::get(ui.ctx());
        let p = tokens.palette;
        let st = tokens.strokes;
        let ts = tokens.type_scale;

        let side = size
            .unwrap_or_else(|| {
                let a = ui.available_size();
                a.x.min(a.y)
            })
            .max(tokens.space(12.0));
        // The disc is square; a readout row sits underneath it so text never collides with the
        // "B" label at small sizes.
        let readout_h = tokens.space(2.2);
        let (full_rect, response) = ui.allocate_exact_size(vec2(side, side + readout_h), Sense::click_and_drag());
        let rect = Rect::from_min_size(full_rect.min, vec2(side, side));
        let c = rect.center();
        let margin = tokens.space(2.5);
        let radius = (side * 0.5 - margin).max(1.0);
        let show_ring_labels = radius >= 110.0;
        let (dmin, dmax) = host.distance_range();
        // Display floor so a very near source never sits on top of the head glyph.
        let min_px = tokens.space(1.4) + tokens.space(1.4) + tokens.space(0.6);
        let dist_to_px = move |d: f32| {
            if distance_enabled {
                (radius * (d / dmax).clamp(0.0, 1.0)).max(min_px.min(radius))
            } else {
                radius * RING_FRACTION
            }
        };
        let px_to_dist = move |r: f32| r / radius * dmax;

        let mods = ui.input(|i| i.modifiers);
        let fine = mods.shift;
        let snapping = mods.alt;
        let mut changed = false;
        let mut selected_other = None;

        let other_pos = |o: &SourceView| c + az_to_dir(o.azimuth) * dist_to_px(o.distance);
        let hit_other = |pos: Pos2| others.iter().position(|o| (other_pos(o) - pos).length() <= tokens.space(1.5));

        // ---------------------------------------------------------------- interaction
        if response.double_clicked() || (response.clicked() && mods.command) {
            for prm in [PosParam::Azimuth, PosParam::Distance] {
                if prm == PosParam::Distance && !distance_enabled {
                    continue;
                }
                let v = host.default_value(prm);
                host.begin_gesture(prm);
                host.set(prm, v);
                host.end_gesture(prm);
            }
            changed = true;
        } else if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(i) = hit_other(pos) {
                    selected_other = Some(i);
                } else {
                    let off = pos - c;
                    let mut az = dir_to_az(off);
                    let mut d = px_to_dist(off.length()).clamp(dmin, dmax);
                    if snapping {
                        az = wrap_deg(snap(az, SNAP_DEG));
                        d = snap(d, SNAP_DIST).clamp(dmin, dmax);
                    }
                    host.begin_gesture(PosParam::Azimuth);
                    host.set(PosParam::Azimuth, az);
                    host.end_gesture(PosParam::Azimuth);
                    if distance_enabled {
                        host.begin_gesture(PosParam::Distance);
                        host.set(PosParam::Distance, d);
                        host.end_gesture(PosParam::Distance);
                    }
                    changed = true;
                }
            }
        }

        if response.drag_started() {
            host.begin_gesture(PosParam::Azimuth);
            if distance_enabled {
                host.begin_gesture(PosParam::Distance);
            }
            let off = response.interact_pointer_pos().unwrap_or(c) - c;
            let state = DragState { last_angle: dir_to_az(off), last_radius: off.length(), az: host.azimuth(), dist: host.distance() };
            ui.data_mut(|d| d.insert_temp(id, state));
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let off = pos - c;
                let ang = dir_to_az(off);
                let rad = off.length();
                let prev = ui
                    .data(|d| d.get_temp::<DragState>(id))
                    .unwrap_or(DragState { last_angle: ang, last_radius: rad, az: host.azimuth(), dist: host.distance() });
                let az = if fine { wrap_deg(prev.az + FINE_SCALE * wrap_deg(ang - prev.last_angle)) } else { ang };
                let dist = if fine {
                    prev.dist + FINE_SCALE * (px_to_dist(rad) - px_to_dist(prev.last_radius))
                } else {
                    px_to_dist(rad)
                }
                .clamp(dmin, dmax);
                let (az_out, dist_out) = if snapping {
                    (wrap_deg(snap(az, SNAP_DEG)), snap(dist, SNAP_DIST).clamp(dmin, dmax))
                } else {
                    (az, dist)
                };
                if az_out != host.azimuth() {
                    host.set(PosParam::Azimuth, az_out);
                    changed = true;
                }
                if distance_enabled && dist_out != host.distance() {
                    host.set(PosParam::Distance, dist_out);
                    changed = true;
                }
                ui.data_mut(|d| d.insert_temp(id, DragState { last_angle: ang, last_radius: rad, az, dist }));
            }
        }
        if response.drag_stopped() {
            host.end_gesture(PosParam::Azimuth);
            if distance_enabled {
                host.end_gesture(PosParam::Distance);
            }
        }

        if response.hovered() && !response.dragged() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            let (left, right) = ui.input(|i| (i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight)));
            let mut delta = 0.0;
            if dy != 0.0 {
                delta += dy * WHEEL_DEG_PER_POINT * if fine { FINE_SCALE } else { 1.0 };
            }
            if left {
                delta += if fine { 0.1 } else { 1.0 };
            }
            if right {
                delta -= if fine { 0.1 } else { 1.0 };
            }
            if delta != 0.0 {
                let az = wrap_deg(host.azimuth() + delta);
                host.begin_gesture(PosParam::Azimuth);
                host.set(PosParam::Azimuth, az);
                host.end_gesture(PosParam::Azimuth);
                changed = true;
            }
        }

        // ---------------------------------------------------------------- drawing
        let painter = ui.painter().with_clip_rect(full_rect);
        painter.circle(c, radius, p.surface, Stroke::new(st.thin, p.surface_alt));

        if distance_enabled {
            for k in 1..=3 {
                let d = dmax * k as f32 / 3.0;
                let r = dist_to_px(d);
                if k < 3 {
                    painter.circle_stroke(c, r, Stroke::new(st.hairline, p.surface_alt));
                }
                if show_ring_labels {
                    let lp = c + az_to_dir(-135.0) * r;
                    painter.text(lp + vec2(tokens.space(0.4), tokens.space(0.2)), Align2::LEFT_TOP, format!("{d:.0} m"), tokens.font(ts.caption), p.ink_muted);
                }
            }
        } else {
            painter.circle_stroke(c, radius * RING_FRACTION, Stroke::new(st.thin, p.surface_alt));
        }

        for (az, txt) in [(0.0, "F"), (90.0, "L"), (-90.0, "R"), (180.0, "B")] {
            let dir = az_to_dir(az);
            painter.line_segment([c + dir * (radius - tokens.space(1.0)), c + dir * radius], Stroke::new(st.thin, p.ink_muted));
            painter.text(c + dir * (radius + margin * 0.55), Align2::CENTER_CENTER, txt, tokens.font(ts.caption), p.ink_muted);
        }
        for az in [45.0, 135.0, -45.0, -135.0] {
            let dir = az_to_dir(az);
            painter.line_segment([c + dir * (radius - tokens.space(0.5)), c + dir * radius], Stroke::new(st.hairline, p.ink_muted));
        }

        // listener head, rotated by the tracked yaw when present
        let yaw = head_yaw.unwrap_or(0.0);
        let hr = tokens.space(1.4);
        painter.circle(c, hr, p.surface_alt, Stroke::new(st.regular, p.ink));
        let nose = az_to_dir(yaw);
        painter.line_segment([c + nose * hr, c + nose * (hr + tokens.space(0.8))], Stroke::new(st.regular, p.ink));
        for ear in [90.0, -90.0] {
            let d = az_to_dir(yaw + ear);
            painter.line_segment([c + d * hr * 0.8, c + d * (hr + tokens.space(0.35))], Stroke::new(st.regular, p.ink));
        }
        if head_yaw.is_some() {
            painter.circle_stroke(c, hr + tokens.space(0.6), Stroke::new(st.hairline, p.accent_soft));
        }

        for o in others {
            let pos = other_pos(o);
            let col = tokens.source_color(o.color_index);
            let r = tokens.space(1.1);
            painter.line_segment([c, pos], Stroke::new(st.hairline, col.gamma_multiply(0.35)));
            level_arc(&painter, pos, r + tokens.space(0.4), o.level, Stroke::new(st.thin, col.gamma_multiply(0.8)));
            painter.circle(pos, r, col.gamma_multiply(0.55), Stroke::new(st.thin, if o.clipping { p.danger } else { p.bg }));
            painter.text(pos, Align2::CENTER_CENTER, &o.label, tokens.font(ts.caption * 0.9), p.bg);
        }

        let az = host.azimuth();
        let el = host.elevation();
        let dist = host.distance();
        let pos = c + az_to_dir(az) * dist_to_px(dist);
        let col = tokens.source_color(color_index);
        let r_dot = tokens.space(1.4);
        painter.line_segment([c, pos], Stroke::new(st.thin, p.accent_soft));
        if el.abs() > 0.5 {
            let len = tokens.space(2.5) * (el.abs() / 90.0);
            let dir = if el > 0.0 { vec2(0.0, -1.0) } else { vec2(0.0, 1.0) };
            let tip = pos + dir * (r_dot + len);
            painter.line_segment([pos + dir * r_dot, tip], Stroke::new(st.regular, col));
            painter.circle_filled(tip, st.regular, col);
        }
        level_arc(&painter, pos, r_dot + tokens.space(0.6), level, Stroke::new(st.bold, col));
        if response.hovered() || response.dragged() {
            painter.circle_stroke(pos, r_dot + tokens.space(1.3), Stroke::new(st.thin, p.accent_soft));
        }
        painter.circle(pos, r_dot, col, Stroke::new(if clipping { st.bold } else { st.thin }, if clipping { p.danger } else { p.bg }));
        painter.text(pos, Align2::CENTER_CENTER, label, tokens.font(ts.caption), p.bg);

        let readout = format!("{}   {}   {}", format_azimuth(az), format_elevation(el), format_distance(dist));
        let readout_pos = pos2(full_rect.left() + margin * 0.4, full_rect.bottom() - readout_h * 0.5);
        painter.text(readout_pos, Align2::LEFT_CENTER, readout, tokens.mono(ts.caption), p.ink);
        if side > 320.0 && response.hovered() && !response.dragged() {
            painter.text(
                pos2(full_rect.right() - margin * 0.4, full_rect.bottom() - readout_h * 0.5),
                Align2::RIGHT_CENTER,
                "shift fine · alt snap · dbl-click reset",
                tokens.font(ts.caption),
                p.ink_muted,
            );
        }

        let dragging = response.dragged();
        let response = response.on_hover_cursor(if dragging { CursorIcon::Grabbing } else { CursorIcon::Grab });
        PadResponse { response, changed, selected_other }
    }
}

/// Vertical elevation control, −90° at the bottom, +90° at the top.
pub struct ElevationStrip<'a> {
    id: Id,
    host: &'a mut dyn PositionHost,
    others: &'a [SourceView],
    color_index: usize,
    height: Option<f32>,
}

impl<'a> ElevationStrip<'a> {
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, host: &'a mut dyn PositionHost) -> Self {
        Self { id: Id::new(id), host, others: &[], color_index: 0, height: None }
    }

    pub fn others(mut self, others: &'a [SourceView]) -> Self {
        self.others = others;
        self
    }

    pub fn color_index(mut self, index: usize) -> Self {
        self.color_index = index;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Width the strip will take, so callers can lay out around it.
    pub fn width(tokens: &Tokens) -> f32 {
        tokens.space(6.5)
    }

    /// Height of the readout row [`PositionPad`] adds under its disc, so a strip can be sized to
    /// match the pad's full height.
    pub fn pad_readout_height(tokens: &Tokens) -> f32 {
        tokens.space(2.2)
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let ElevationStrip { id, host, others, color_index, height } = self;
        let tokens = Tokens::get(ui.ctx());
        let p = tokens.palette;
        let st = tokens.strokes;
        let ts = tokens.type_scale;

        let h = height.unwrap_or_else(|| ui.available_size().y).max(tokens.space(10.0));
        let w = Self::width(&tokens);
        let (rect, response) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
        let margin = tokens.space(1.5);
        let track = Rect::from_center_size(pos2(rect.right() - tokens.space(1.6), rect.center().y), vec2(tokens.space(1.5), h - 2.0 * margin));
        let y_of = move |el: f32| track.center().y - (el / 90.0) * track.height() * 0.5;
        let el_of = move |y: f32| ((track.center().y - y) / (track.height() * 0.5) * 90.0).clamp(-90.0, 90.0);

        let mods = ui.input(|i| i.modifiers);
        let fine = mods.shift;
        let snapping = mods.alt;

        // ---------------------------------------------------------------- interaction
        if response.double_clicked() || (response.clicked() && mods.command) {
            let v = host.default_value(PosParam::Elevation);
            host.begin_gesture(PosParam::Elevation);
            host.set(PosParam::Elevation, v);
            host.end_gesture(PosParam::Elevation);
        } else if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let mut el = el_of(pos.y);
                if snapping {
                    el = snap(el, SNAP_DEG).clamp(-90.0, 90.0);
                }
                host.begin_gesture(PosParam::Elevation);
                host.set(PosParam::Elevation, el);
                host.end_gesture(PosParam::Elevation);
            }
        }
        if response.drag_started() {
            host.begin_gesture(PosParam::Elevation);
            let y = response.interact_pointer_pos().map(|p| p.y).unwrap_or(track.center().y);
            ui.data_mut(|d| d.insert_temp(id, StripDrag { last_y: y, el: host.elevation() }));
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let prev = ui.data(|d| d.get_temp::<StripDrag>(id)).unwrap_or(StripDrag { last_y: pos.y, el: host.elevation() });
                let el = if fine { prev.el + FINE_SCALE * (el_of(pos.y) - el_of(prev.last_y)) } else { el_of(pos.y) }.clamp(-90.0, 90.0);
                let el_out = if snapping { snap(el, SNAP_DEG).clamp(-90.0, 90.0) } else { el };
                if el_out != host.elevation() {
                    host.set(PosParam::Elevation, el_out);
                }
                ui.data_mut(|d| d.insert_temp(id, StripDrag { last_y: pos.y, el }));
            }
        }
        if response.drag_stopped() {
            host.end_gesture(PosParam::Elevation);
        }
        if response.hovered() && !response.dragged() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            let (up, down) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
            let mut delta = 0.0;
            if dy != 0.0 {
                delta -= dy * WHEEL_DEG_PER_POINT * if fine { FINE_SCALE } else { 1.0 };
            }
            if up {
                delta += if fine { 0.1 } else { 1.0 };
            }
            if down {
                delta -= if fine { 0.1 } else { 1.0 };
            }
            if delta != 0.0 {
                let el = (host.elevation() + delta).clamp(-90.0, 90.0);
                host.begin_gesture(PosParam::Elevation);
                host.set(PosParam::Elevation, el);
                host.end_gesture(PosParam::Elevation);
            }
        }

        // ---------------------------------------------------------------- drawing
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(track, CornerRadius::same(tokens.radii.sm), p.surface);
        painter.rect_stroke(track, CornerRadius::same(tokens.radii.sm), Stroke::new(st.hairline, p.surface_alt), StrokeKind::Inside);
        for el in [-90.0, -60.0, -30.0, 0.0, 30.0, 60.0, 90.0f32] {
            let y = y_of(el);
            let len = if el % 90.0 == 0.0 { tokens.space(0.8) } else { tokens.space(0.4) };
            painter.line_segment([pos2(track.left() - len, y), pos2(track.left(), y)], Stroke::new(st.hairline, p.ink_muted));
        }
        for (el, txt) in [(90.0, "+90"), (0.0, "0"), (-90.0, "−90")] {
            painter.text(pos2(track.left() - tokens.space(1.1), y_of(el)), Align2::RIGHT_CENTER, txt, tokens.font(ts.caption), p.ink_muted);
        }

        let el = host.elevation();
        let (y0, y1) = (y_of(0.0), y_of(el));
        let fill = Rect::from_min_max(pos2(track.left(), y0.min(y1)), pos2(track.right(), y0.max(y1)));
        painter.rect_filled(fill, CornerRadius::ZERO, p.accent_soft);
        for o in others {
            let y = y_of(o.elevation);
            painter.line_segment([pos2(track.left(), y), pos2(track.right(), y)], Stroke::new(st.thin, tokens.source_color(o.color_index).gamma_multiply(0.6)));
        }
        let m = pos2(track.center().x, y1);
        let col = tokens.source_color(color_index);
        if response.hovered() || response.dragged() {
            painter.circle_stroke(m, tokens.space(1.6), Stroke::new(st.thin, p.accent_soft));
        }
        painter.circle(m, tokens.space(1.0), col, Stroke::new(st.thin, p.bg));

        let dragging = response.dragged();
        response.on_hover_cursor(if dragging { CursorIcon::Grabbing } else { CursorIcon::Grab })
    }
}

fn level_arc(painter: &egui::Painter, center: Pos2, r: f32, level: f32, stroke: Stroke) {
    let level = level.clamp(0.0, 1.0);
    if level <= 0.01 {
        return;
    }
    let n = (level * 48.0).ceil().max(2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = -PI / 2.0 + (k as f32 / n as f32) * level * TAU;
            center + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.line(pts, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_is_canonical() {
        assert_eq!(wrap_deg(190.0), -170.0);
        assert_eq!(wrap_deg(-190.0), 170.0);
        assert_eq!(wrap_deg(180.0), 180.0);
        assert_eq!(wrap_deg(-180.0), 180.0);
        assert_eq!(wrap_deg(0.0), 0.0);
    }

    #[test]
    fn direction_round_trip() {
        for az in [-179.0f32, -90.0, -45.0, 0.0, 30.0, 90.0, 135.0, 180.0] {
            let back = dir_to_az(az_to_dir(az));
            assert!((wrap_deg(back - az)).abs() < 1e-3, "az {az} -> {back}");
        }
        // front is up on screen, left is left on screen
        assert!(az_to_dir(0.0).y < -0.99);
        assert!(az_to_dir(90.0).x < -0.99);
        assert!(az_to_dir(-90.0).x > 0.99);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(37.0, SNAP_DEG), 30.0);
        assert_eq!(snap(38.0, SNAP_DEG), 45.0);
        assert_eq!(snap(1.1, SNAP_DIST), 1.0);
    }

    #[test]
    fn readout_hides_sign_convention() {
        assert_eq!(format_azimuth(30.0), "30° L");
        assert_eq!(format_azimuth(-30.0), "30° R");
        assert_eq!(format_azimuth(0.0), "0° F");
        assert_eq!(format_azimuth(180.0), "180° B");
        assert_eq!(format_elevation(10.0), "+10°");
        assert_eq!(format_elevation(-10.0), "−10°");
        assert_eq!(format_distance(1.0), "1.00 m");
    }
}
