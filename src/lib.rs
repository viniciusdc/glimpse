//! Glimpse — an animated GIF screen recorder built around a framing window.
//!
//! The library half exists so the geometry chain can be tested and demonstrated
//! without launching the application. See `examples/` and `tests/`.
//!
//! X11-only by design, not by omission — see
//! [ADR 0002](../docs/adr/0002-ffmpeg-pipeline-and-session-model.md).

pub mod geometry;
pub mod session;
pub mod ui;
pub mod x11probe;
