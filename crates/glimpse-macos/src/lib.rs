//! The macOS side of Glimpse.
//!
//! Two halves, split by what needs a toolkit:
//!
//! * [`grab`] and [`geometry`] are plain Rust. Turning a rectangle into
//!   `avfoundation` arguments is string building, and the AppKit coordinate flip
//!   is arithmetic, so both compile and are tested on every platform. That is
//!   deliberate — it means Linux CI guards the two things most likely to be
//!   silently wrong on a machine nobody is looking at.
//! * [`window`] needs AppKit and GTK, and exists only on macOS.
//!
//! The frame is several windows rather than one, because GTK cannot make a
//! covered region click-through — see
//! [ADR 0011](../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).

pub mod geometry;
pub mod grab;

#[cfg(target_os = "macos")]
pub mod window;

pub use grab::AvfCapture;
