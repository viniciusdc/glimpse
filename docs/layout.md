# Project layout

Every path here is checked by [`scripts/sync-docs.sh`](../scripts/sync-docs.sh):
each one must exist, none may be listed twice, and every Rust source in the
workspace must appear. The descriptions are hand-written — edit them freely.


<!-- BEGIN GENERATED layout (verified by scripts/sync-docs.sh; edit descriptions freely, paths are checked) -->
```
AGENTS.md               Working agreement — the failure modes already hit here
Makefile                make check is the gate; make selftest is the one CI can't run
src/
  main.rs               The binary: CLI, and picking a frontend by target
crates/glimpse-core/    Platform-free. No gtk4, no x11rb, no objc2 — by manifest
  src/lib.rs            Library surface, so the logic is testable without a display
  src/geometry.rs       The capture rect, and the coordinate convention it carries
  src/config.rs         Persisted settings: theme, format, output folder
  src/session.rs        The recording lifecycle: pure state machine, no I/O
  src/capture.rs        The ffmpeg recorder: owns the child, reaps on every path
  src/worker.rs         Runs the recorder off the UI thread; dropping it reaps
  src/encode.rs         GIF and MP4 encoding, and the atomic commit
  tests/geometry.rs     Clipping — the part of the chain testable without a display
  tests/session.rs      Lifecycle policy: drift, retry, cancellation, shutdown
  tests/capture.rs      Output arguments, filter placement, workspace ownership
  tests/config.rs       Defaults, round-trip, and surviving a corrupt file
  tests/encode.rs       Collision policy, argument shape, real GIF and MP4 encodes
  tests/progress_probe.rs  What ffmpeg's progress output actually looks like
crates/glimpse-ui/      The chrome: palette, stylesheet, formatters, the platform seam
  src/lib.rs            Shared between frontends; the window model is not
  src/hooks.rs          PlatformHooks: the four things the chrome needs from a platform
crates/glimpse-macos/   The macOS side: capture backend, and the frame
  src/lib.rs            Surface, split by what needs a toolkit
  src/grab.rs           Rect → avfoundation arguments, and the screen device lookup
  src/geometry.rs       The AppKit → capture-rect flip. No AppKit, so tested everywhere
  src/layout.rs         Where the two windows go. Also toolkit-free, also tested
  src/window.rs         Reaching through GTK to the NSWindow; placement, lockstep
  src/frame.rs          Two windows; the frame one takes no clicks at all
  src/app.rs            Application entry: put the frame up, report its rect
  examples/record.rs    Record a fixed region end to end, no window involved
  examples/frame.rs     Show the frame and read its geometry back from the server
crates/glimpse-x11/     The X11 frontend: GTK4 window, punched input region
  src/lib.rs            Frontend surface
  src/app.rs            Application entry, startup refusals, stale-state sweeps
  src/ui.rs             The framing window: hole, input region, lock/unlock
  src/x11probe.rs       The X11 boundary — origin, root size, input-shape readback
  src/geometry.rs       WidgetRect → SurfaceRect → ScreenPixelRect, with clipping
  src/grab.rs           Rect → x11grab arguments; the input half of the seam
  tests/grab.rs         The x11grab flags, each one a licensing commitment
  examples/root_geometry.rs    Query X with no GTK window involved
  examples/framing_window.rs   The smallest useful framing window
  examples/record.rs           Record a fixed region, no GTK window involved
data/
  glimpse.desktop       Desktop entry, installed by `make install`
scripts/
  headless.sh           Runs Glimpse on a private X server, off your screen
  smoke.sh              Drives one user journey and turns it into an exit status
  selftest.sh           Geometry and input region, with a status the suite can read
  check-journeys.sh     Fails if a journey exists that nothing drives
  check-links-external.sh  External links in the docs; only a 404 fails
  install.sh            Installs a release, refusing anything that fails its checksum
  make-demo.py          Draws the README animation, frame by frame
  sync-docs.sh          Regenerates the ADR index; fails the build on doc drift
docs/
  adr/                  Decision records, append-only
  releasing.md          Cutting a release and verifying it
  faq.md                Using it: output, formats, aborted recordings, platforms
  assets/               README banner, light and dark
  architecture.md       The stack, the modules, the conversion chain
  development.md        Setting up, the gates, how to verify geometry changes
  roadmap.md            What comes next, in dependency order
```
<!-- END GENERATED layout -->
