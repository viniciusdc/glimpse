//! The macOS capture backend of Glimpse.
//!
//! Today this is argument construction and nothing else: a `ScreenPixelRect`
//! becomes an `avfoundation` invocation that `glimpse-core` can spawn. There is
//! no window model here yet, and no AppKit dependency, which is why the crate
//! builds and its tests run on Linux CI as well.
//!
//! The frame that will eventually produce those rectangles is described in
//! [ADR 0011](../docs/adr/0011-why-the-macos-frame-is-more-than-one-window.md).

pub mod grab;

pub use grab::AvfCapture;
