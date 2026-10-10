//! Framework-agnostic egui widgets and design tokens for the spatializer UI.
//!
//! The plugin hosts these through nice-plug-egui (or nih_plug_egui); the standalone example runs
//! them under eframe with the same code. See `../UI.md` for the design rationale.

pub mod position;
pub mod tokens;

pub use position::{
    format_azimuth, format_distance, format_elevation, ElevationStrip, PadResponse, PosParam, PositionHost, PositionPad, SourceView,
};
pub use tokens::{Palette, Radii, Strokes, ThemeMode, Tokens, TypeScale};
