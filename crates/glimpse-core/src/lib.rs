//! The platform-free core of Glimpse.
//!
//! Everything here is deliberately blind to how pixels are framed on screen and
//! to which backend grabs them. It owns the recording lifecycle, the ffmpeg child
//! and its reaping, encoding, configuration, and the capture rectangle type that
//! every frontend must produce.
//!
//! The boundary is enforced by this crate's manifest rather than by convention:
//! there is no `gtk4`, no `gdk4-*`, no `x11rb` and no `objc2` in it, so a module
//! here *cannot* reach for a toolkit. See
//! [ADR 0010](../docs/adr/0010-capture-providers-and-a-platform-free-core.md).
//!
//! What crosses the seam is [`capture::GrabCommand`] — a backend's answer to "how
//! do I grab that rectangle?", as pure data.

pub mod capture;
pub mod config;
pub mod encode;
pub mod geometry;
pub mod session;
pub mod worker;
