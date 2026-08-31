# Architecture decision records

One file per decision, in the order they were taken. Append-only: a record that
turned out to be wrong is **superseded by a new one**, not edited, because the
reasoning that failed is the part worth keeping.

Several here exist only because something measured contradicted something
assumed. [ADR 0011](0011-why-the-macos-frame-is-more-than-one-window.md) is the
clearest case — its measurements stand and its conclusion does not, and
[ADR 0015](0015-the-frame-is-two-windows.md) says so rather than quietly
replacing it.

<!-- BEGIN GENERATED adr-index (regenerate with `make docs-sync`) -->
  - [0000](0000-x11-framing-window-spike.md) — The X11 framing-window spike
  - [0001](0001-rust-and-gtk4.md) — Rust and GTK4 as the stack
  - [0002](0002-ffmpeg-pipeline-and-session-model.md) — An ffmpeg-only pipeline, and an explicit session model
  - [0003](0003-apache-2-0.md) — Apache-2.0, and what that requires of the capture implementation
  - [0004](0004-review-corrections-and-the-lifecycle-spine.md) — Review corrections, and a lifecycle spine before capture
  - [0005](0005-gif-encoding-and-the-atomic-commit.md) — GIF encoding, and how the output is committed
  - [0006](0006-the-header-is-the-chrome.md) — The header bar is the window chrome
  - [0007](0007-gif-and-mp4.md) — GIF and MP4 as the initial output formats
  - [0008](0008-settings-and-themes.md) — Settings, and what the theme is allowed to change
  - [0009](0009-snapshot.md) — Snapshot, and why it is not a one-frame recording
  - [0010](0010-capture-providers-and-a-platform-free-core.md) — Capture providers, and a platform-free core
  - [0011](0011-why-the-macos-frame-is-more-than-one-window.md) — Why the macOS frame is more than one window
  - [0012](0012-a-setting-a-backend-cannot-honour.md) — A setting a backend cannot honour is not offered
  - [0013](0013-macos-ships-an-app-bundle.md) — macOS ships an `.app` bundle
  - [0014](0014-the-chrome-is-shared-the-window-model-is-not.md) — The chrome is shared, the window model is not
  - [0015](0015-the-frame-is-two-windows.md) — The frame is two windows, and one of them takes no clicks
  - [0016](0016-the-chrome-is-above-and-below.md) — The macOS chrome is above *and* below the frame
<!-- END GENERATED adr-index -->
