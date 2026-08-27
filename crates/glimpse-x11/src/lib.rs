//! The X11 frontend of Glimpse: a GTK4 framing window with a hole punched in its
//! input region, and the `x11grab` backend that records through it.
//!
//! Everything platform-shaped lives on this side of ADR 0010's seam — the window
//! model, the widget-to-pixels chain, the X server probe, and the construction of
//! the ffmpeg input arguments. `glimpse-core` owns the lifecycle and the child
//! process, and cannot name any of it.
//!
//! X11 by design, not by omission — see
//! [ADR 0002](../docs/adr/0002-ffmpeg-pipeline-and-session-model.md).

pub mod app;
pub mod geometry;
pub mod grab;
pub mod ui;
pub mod x11probe;

pub use app::run;
