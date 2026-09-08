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
//! The frame is **one** window, and it stops taking clicks for as long as the
//! user needs to reach what is behind it
//! ([ADR 0017](../docs/adr/0017-click-through-is-a-mode-not-a-window.md)). GTK
//! still cannot make a covered region click-through on macOS — that measurement
//! from [ADR 0011](../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md)
//! stands, and was widened rather than overturned — but a hole only has to pass
//! clicks while somebody is clicking through it.

pub mod geometry;
pub mod grab;
pub mod shortcut;
pub mod stop;

#[cfg(target_os = "macos")]
pub mod app;
#[cfg(target_os = "macos")]
pub mod hotkey;
#[cfg(target_os = "macos")]
pub mod menubar;
#[cfg(target_os = "macos")]
pub mod ui;
#[cfg(target_os = "macos")]
pub mod window;

#[cfg(target_os = "macos")]
pub use app::run;

pub use grab::AvfCapture;
