//! Plugin editor: hosts the `spatializer-ui` widgets inside the DAW.
//!
//! `nih_plug_egui` (at the pinned nih-plug commit) is built on egui 0.31, while the widget crate
//! targets egui 0.36, so this module implements nih-plug's [`Editor`] trait directly on top of
//! egui-baseview 0.7 (egui 0.36). It is the same job `nih_plug_egui::create_egui_editor` does:
//! open a child window in the host's parent window, draw with egui, and route every parameter
//! change through [`ParamSetter`] with begin/end gestures so hosts record automation correctly.

use std::any::Any;
use std::ffi::c_void;
use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use egui::{pos2, vec2, Align2, CornerRadius, Margin, Rect, RichText, Sense, Stroke, Ui};
use egui_baseview::baseview::dpi::{LogicalSize, PhysicalSize, Size};
use egui_baseview::baseview::{HandlerError, Window};
use egui_baseview::{EguiWindow, EguiWindowSettings, KeyCapture, RepaintNotifier};
use nih_plug::prelude::*;
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
    XcbWindowHandle,
};
use spatializer_ui::position::FINE_SCALE;
use spatializer_ui::{ElevationStrip, PosParam, PositionHost, PositionPad, SourceView, Tokens};

use crate::SpatializerParams;

/// Editor size in logical pixels. The pad layout is responsive, so this is only the default.
pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 440;

/// State shared between the audio thread and the editor.
pub struct EditorShared {
    open: AtomicBool,
    repaint: RepaintNotifier,
}

impl Default for EditorShared {
    fn default() -> Self {
        Self { open: AtomicBool::new(false), repaint: RepaintNotifier::new() }
    }
}

impl EditorShared {
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    /// Realtime-safe: only sets a flag the window polls on its frame timer.
    pub fn request_repaint(&self) {
        self.repaint.request_repaint();
    }
}

pub fn create(
    params: Arc<SpatializerParams>,
    peak_meter: Arc<AtomicF32>,
    shared: Arc<EditorShared>,
) -> Option<Box<dyn Editor>> {
    Some(Box::new(SpatializerEditor {
        params,
        peak_meter,
        shared,
        // Hosts on Windows and Linux report their scale through `set_scale_factor()`; until they
        // do, assume 1.0 like nih_plug_egui. macOS works in logical pixels throughout.
        scale_factor: AtomicF32::new(1.0),
    }))
}

struct SpatializerEditor {
    params: Arc<SpatializerParams>,
    peak_meter: Arc<AtomicF32>,
    shared: Arc<EditorShared>,
    scale_factor: AtomicF32,
}

impl Editor for SpatializerEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        // The host sizes its frame as `size() * scale factor`, so fill exactly that many pixels.
        let size: Size = if cfg!(target_os = "macos") {
            LogicalSize::new(WIDTH as f64, HEIGHT as f64).into()
        } else {
            let scale = self.scale_factor.load(Ordering::Relaxed) as f64;
            PhysicalSize::new((WIDTH as f64 * scale).round() as u32, (HEIGHT as f64 * scale).round() as u32).into()
        };
        let settings = EguiWindowSettings::new()
            .with_title("Spatializer")
            .with_size(size)
            .with_resizable(false)
            .with_parent(Some(&ParentWindow(parent)))
            .with_repaint_notifier(self.shared.repaint.clone());
        let app = SpatializerApp {
            params: self.params.clone(),
            peak_meter: self.peak_meter.clone(),
            context,
        };

        let window = match EguiWindow::create(settings, app) {
            Ok(window) => {
                if let Err(err) = window.show() {
                    nih_error!("failed to show the editor window: {err}");
                }
                Some(window)
            }
            Err(err) => {
                nih_error!("failed to create the editor window: {err}");
                None
            }
        };

        self.shared.open.store(true, Ordering::Release);
        self.shared.request_repaint();
        Box::new(EditorHandle { shared: self.shared.clone(), _window: window })
    }

    fn size(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // The window cannot be rescaled while open (same restriction as nih_plug_egui).
        if self.shared.is_open() {
            return false;
        }
        self.scale_factor.store(factor, Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.shared.request_repaint();
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.shared.request_repaint();
    }

    fn param_values_changed(&self) {
        self.shared.request_repaint();
    }
}

/// Dropped by the wrapper when the host closes the editor; dropping the window destroys it.
struct EditorHandle {
    shared: Arc<EditorShared>,
    _window: Option<Window>,
}

/// The baseview window handle is `!Send` because it wraps raw platform pointers. nih-plug creates
/// and drops editor handles on the host's GUI thread, the same contract nih_plug_egui relies on.
unsafe impl Send for EditorHandle {}

impl Drop for EditorHandle {
    fn drop(&mut self) {
        self.shared.open.store(false, Ordering::Release);
    }
}

/// nih-plug's parent handle speaks raw-window-handle 0.5; baseview 0.3 wants 0.6.
struct ParentWindow(ParentWindowHandle);

impl HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match self.0 {
            ParentWindowHandle::X11Window(window) => {
                RawWindowHandle::Xcb(XcbWindowHandle::new(NonZeroU32::new(window).ok_or(HandleError::Unavailable)?))
            }
            ParentWindowHandle::AppKitNsView(ns_view) => RawWindowHandle::AppKit(AppKitWindowHandle::new(
                NonNull::new(ns_view as *mut c_void).ok_or(HandleError::Unavailable)?,
            )),
            ParentWindowHandle::Win32Hwnd(hwnd) => RawWindowHandle::Win32(Win32WindowHandle::new(
                NonZeroIsize::new(hwnd as isize).ok_or(HandleError::Unavailable)?,
            )),
        };
        // Safety: the host keeps the parent window alive for as long as the editor is attached.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

struct SpatializerApp {
    params: Arc<SpatializerParams>,
    peak_meter: Arc<AtomicF32>,
    context: Arc<dyn GuiContext>,
}

impl egui_baseview::App for SpatializerApp {
    fn build(&mut self, egui_ctx: egui::Context, frame: &mut egui_baseview::Frame) -> Result<(), HandlerError> {
        let tokens = Tokens::dark();
        tokens.install(&egui_ctx);
        frame.set_clear_color(tokens.palette.bg.into());
        // Leave the keyboard to the host so transport shortcuts (space bar) keep working while
        // the editor has focus. Nothing in this UI needs typed input.
        frame.set_key_capture(KeyCapture::IgnoreAll);
        Ok(())
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut egui_baseview::Frame) {
        let setter = ParamSetter::new(self.context.as_ref());
        let peak = self.peak_meter.load(Ordering::Relaxed);
        let tokens = Tokens::get(ui.ctx());
        let bg = tokens.palette.bg;
        let margin = tokens.space(2.0) as i8;

        egui::Panel::top("header")
            .frame(egui::Frame::NONE.fill(bg).inner_margin(Margin { left: margin, right: margin, top: margin, bottom: 0 }))
            .show(ui, |ui| header(ui, &tokens));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg).inner_margin(Margin::same(margin)))
            .show(ui, |ui| body(ui, &tokens, &self.params, &setter, peak));
    }
}

/// Routes the position widgets to the plugin parameters.
struct ParamHost<'a> {
    params: &'a SpatializerParams,
    setter: &'a ParamSetter<'a>,
}

impl ParamHost<'_> {
    fn param(&self, param: PosParam) -> Option<&FloatParam> {
        match param {
            PosParam::Azimuth => Some(&self.params.azimuth),
            PosParam::Elevation => Some(&self.params.elevation),
            // The dataset has a single radius; distance is not a parameter yet.
            PosParam::Distance => None,
        }
    }
}

impl PositionHost for ParamHost<'_> {
    fn azimuth(&self) -> f32 {
        self.params.azimuth.value()
    }

    fn elevation(&self) -> f32 {
        self.params.elevation.value()
    }

    fn distance(&self) -> f32 {
        1.0
    }

    fn set(&mut self, param: PosParam, value: f32) {
        if let Some(p) = self.param(param) {
            self.setter.set_parameter(p, value);
        }
    }

    fn begin_gesture(&mut self, param: PosParam) {
        if let Some(p) = self.param(param) {
            self.setter.begin_set_parameter(p);
        }
    }

    fn end_gesture(&mut self, param: PosParam) {
        if let Some(p) = self.param(param) {
            self.setter.end_set_parameter(p);
        }
    }

    fn default_value(&self, param: PosParam) -> f32 {
        self.param(param).map_or(1.0, |p| p.default_plain_value())
    }
}

fn header(ui: &mut Ui, tokens: &Tokens) {
    let p = tokens.palette;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Spatializer").size(tokens.type_scale.title).color(p.ink).strong());
        ui.add_space(tokens.space(1.0));
        for text in ["HRTF  KU100 2°", "Stereo · binaural"] {
            egui::Frame::NONE
                .fill(p.surface)
                .stroke(Stroke::new(tokens.strokes.hairline, p.surface_alt))
                .corner_radius(CornerRadius::same(tokens.radii.md))
                .inner_margin(Margin::symmetric(tokens.space(1.0) as i8, (tokens.space(0.4)) as i8))
                .show(ui, |ui| ui.label(RichText::new(text).size(tokens.type_scale.caption).color(p.ink_muted)));
        }
    });
}

fn body(ui: &mut Ui, tokens: &Tokens, params: &SpatializerParams, setter: &ParamSetter, peak: f32) {
    let avail = ui.available_size();
    let gap = ui.spacing().item_spacing.x;
    let rail_w = tokens.space(22.0);
    let strip_w = ElevationStrip::width(tokens);
    let readout_h = ElevationStrip::pad_readout_height(tokens);
    let side = (avail.x - strip_w - rail_w - 2.0 * gap).min(avail.y - readout_h).max(tokens.space(12.0));

    // Where the left and right input channels actually end up.
    let [(left_az, el), (right_az, _)] = params.position().sources();
    let ghost = |label: &str, azimuth: f32, color_index: usize| SourceView {
        label: label.into(),
        azimuth,
        elevation: el,
        distance: 1.0,
        level: 0.0,
        clipping: false,
        color_index,
    };
    let ghosts = [ghost("L", left_az, 1), ghost("R", right_az, 2)];

    ui.horizontal_top(|ui| {
        let mut host = ParamHost { params, setter };
        PositionPad::new("position", &mut host)
            .others(&ghosts)
            .select_others(false)
            .label("")
            .level(meter_level(peak))
            .clipping(peak >= 1.0)
            .distance_enabled(false)
            .size(side)
            .show(ui);
        ElevationStrip::new("elevation", &mut host).height(side + readout_h).show(ui);
        ui.vertical(|ui| {
            ui.set_width(rail_w);
            rail(ui, tokens, params, setter, peak);
        });
    });
}

fn rail(ui: &mut Ui, tokens: &Tokens, params: &SpatializerParams, setter: &ParamSetter, peak: f32) {
    let p = tokens.palette;
    let section = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(tokens.type_scale.caption).color(p.ink_muted).strong());
    };

    section(ui, "STEREO");
    param_slider(ui, tokens, setter, &params.width, "Width");
    ui.label(
        RichText::new("Spread between the L and R channels. 0° collapses to mono.")
            .size(tokens.type_scale.caption)
            .color(p.ink_muted),
    );
    ui.add_space(tokens.space(1.0));

    section(ui, "OUTPUT");
    param_slider(ui, tokens, setter, &params.gain, "Gain");
    level_meter(ui, tokens, peak);

    ui.add_space(tokens.space(1.0));
    ui.label(
        RichText::new("Drag to place · Shift fine · Alt snap · Double-click reset")
            .size(tokens.type_scale.caption)
            .color(p.ink_muted),
    );
}

/// −60…0 dBFS mapped to 0…1.
fn meter_level(peak: f32) -> f32 {
    ((util::gain_to_db(peak) + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn level_meter(ui: &mut Ui, tokens: &Tokens, peak: f32) {
    let p = tokens.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), tokens.space(3.0)), Sense::hover());
    let painter = ui.painter_at(rect);
    let bar = Rect::from_min_size(pos2(rect.left(), rect.bottom() - tokens.space(0.75)), vec2(rect.width(), tokens.space(0.75)));
    let radius = CornerRadius::same(tokens.radii.sm);
    painter.rect_filled(bar, radius, p.surface_alt);
    let level = meter_level(peak);
    if level > 0.0 {
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * level, bar.height()));
        painter.rect_filled(fill, radius, if peak >= 1.0 { p.danger } else { p.accent });
    }
    let db = util::gain_to_db(peak);
    let text = if db > -100.0 { format!("{db:.1} dBFS") } else { "−∞ dBFS".to_string() };
    painter.text(rect.left_top(), Align2::LEFT_TOP, "Level", tokens.font(tokens.type_scale.label), p.ink_muted);
    painter.text(rect.right_top(), Align2::RIGHT_TOP, text, tokens.mono(tokens.type_scale.label), p.ink);
}

/// Horizontal parameter slider with the same interaction rules as the position widgets: drag,
/// click to place, Shift for fine (relative, no jump), double-click or Ctrl/Cmd-click to reset,
/// wheel to nudge. Every change is wrapped in a begin/end gesture.
fn param_slider(ui: &mut Ui, tokens: &Tokens, setter: &ParamSetter, param: &FloatParam, label: &str) {
    let p = tokens.palette;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), tokens.space(4.0)), Sense::click_and_drag());
    let id = response.id;
    let knob_r = tokens.space(0.9);
    let track_y = rect.bottom() - knob_r - 1.0;
    let track = Rect::from_min_max(
        pos2(rect.left() + knob_r, track_y - tokens.space(0.3)),
        pos2(rect.right() - knob_r, track_y + tokens.space(0.3)),
    );
    let norm_at = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0);
    let mods = ui.input(|i| i.modifiers);
    let current = param.unmodulated_normalized_value();

    if response.double_clicked() || (response.clicked() && mods.command) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, norm_at(pos.x));
            setter.end_set_parameter(param);
        }
    }
    if response.drag_started() {
        setter.begin_set_parameter(param);
        let x = response.interact_pointer_pos().map_or(track.center().x, |pos| pos.x);
        ui.data_mut(|d| d.insert_temp(id, (x, current)));
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let (last_x, last_norm) = ui.data(|d| d.get_temp::<(f32, f32)>(id)).unwrap_or((pos.x, current));
            let norm = if mods.shift {
                (last_norm + FINE_SCALE * (pos.x - last_x) / track.width()).clamp(0.0, 1.0)
            } else {
                norm_at(pos.x)
            };
            if norm != current {
                setter.set_parameter_normalized(param, norm);
            }
            ui.data_mut(|d| d.insert_temp(id, (pos.x, norm)));
        }
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if response.hovered() && !response.dragged() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let step = dy * 0.0004 * if mods.shift { FINE_SCALE } else { 1.0 };
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, (current + step).clamp(0.0, 1.0));
            setter.end_set_parameter(param);
        }
    }

    let painter = ui.painter_at(rect);
    let st = tokens.strokes;
    let radius = CornerRadius::same(tokens.radii.sm);
    let value = param.unmodulated_normalized_value();
    let x = track.left() + track.width() * value;
    painter.text(rect.left_top(), Align2::LEFT_TOP, label, tokens.font(tokens.type_scale.label), p.ink_muted);
    painter.text(rect.right_top(), Align2::RIGHT_TOP, param.to_string(), tokens.mono(tokens.type_scale.label), p.ink);
    painter.rect_filled(track, radius, p.surface_alt);
    painter.rect_filled(Rect::from_min_max(track.min, pos2(x, track.max.y)), radius, p.accent_soft);
    let center = pos2(x, track.center().y);
    if response.hovered() || response.dragged() {
        painter.circle_stroke(center, knob_r + tokens.space(0.5), Stroke::new(st.thin, p.accent_soft));
    }
    painter.circle(center, knob_r, p.accent, Stroke::new(st.thin, p.bg));
}
