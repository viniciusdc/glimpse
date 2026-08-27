//! The X11 half of the ffmpeg invocation: how a rectangle reaches `x11grab`.
//!
//! This is the front door of the whole recording pipeline, and every flag in it
//! is a licensing commitment (ADR 0003 — derived from ffmpeg's docs, not from
//! Peek). The output half — codec, container, filter placement — is core's, and
//! is tested there.

use glimpse_core::capture::GrabRequest;
use glimpse_core::geometry::ScreenPixelRect;
use glimpse_x11::grab::X11Capture;

fn request() -> GrabRequest {
    GrabRequest {
        rect: ScreenPixelRect {
            x: 100,
            y: 200,
            w: 640,
            h: 480,
        },
        framerate: Some(15),
        capture_mouse: true,
    }
}

/// The backend reads `DISPLAY` rather than taking one, so a test has to set it.
/// Serialised implicitly by being the only test that touches the environment.
fn backend_with_display(display: &str) -> X11Capture {
    // SAFETY: single-threaded within this test binary's use of it, and the value
    // is read immediately below.
    unsafe { std::env::set_var("DISPLAY", display) };
    X11Capture::from_env().expect("DISPLAY was just set")
}

fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

#[test]
fn the_region_is_passed_as_documented_options_not_baked_into_the_url() {
    // ffmpeg documents -grab_x/-grab_y. Encoding the origin into the input URL
    // works too, but is fragile against display names containing punctuation.
    let cmd = backend_with_display(":0").grab(&request());
    assert_eq!(value_of(&cmd.input, "-grab_x"), Some("100"));
    assert_eq!(value_of(&cmd.input, "-grab_y"), Some("200"));
    assert_eq!(
        value_of(&cmd.input, "-i"),
        Some(":0"),
        "the input is the display alone"
    );
}

#[test]
fn size_and_framerate_come_from_the_request() {
    let cmd = backend_with_display(":0").grab(&request());
    assert_eq!(value_of(&cmd.input, "-video_size"), Some("640x480"));
    assert_eq!(value_of(&cmd.input, "-framerate"), Some("15"));
}

#[test]
fn the_mouse_toggle_maps_to_draw_mouse() {
    let backend = backend_with_display(":0");
    let with = backend.grab(&request());
    assert_eq!(value_of(&with.input, "-draw_mouse"), Some("1"));

    let without = backend.grab(&GrabRequest {
        capture_mouse: false,
        ..request()
    });
    assert_eq!(value_of(&without.input, "-draw_mouse"), Some("0"));
}

#[test]
fn the_display_is_read_from_the_environment_never_guessed() {
    // Guessing :0 could point ffmpeg at a different screen than the one the
    // rectangle was computed against.
    let cmd = backend_with_display(":7.1").grab(&request());
    assert_eq!(value_of(&cmd.input, "-i"), Some(":7.1"));
}

/// x11grab grabs only the region asked for, so a crop filter would apply the
/// region twice. `avfoundation` is the case that needs one.
#[test]
fn x11grab_supplies_no_crop_filter() {
    assert_eq!(backend_with_display(":0").grab(&request()).filter, None);
}

#[test]
fn the_native_pixel_format_is_declared_so_the_intermediate_converts_nothing() {
    let cmd = backend_with_display(":0").grab(&request());
    assert_eq!(cmd.pix_fmt.as_deref(), Some("bgr0"));
}

// ----------------------------------------------------------------- snapshot --

#[test]
fn a_snapshot_sets_no_capture_rate() {
    let cmd = backend_with_display(":0").grab(&GrabRequest {
        framerate: None,
        ..request()
    });
    assert!(
        !cmd.input.iter().any(|a| a == "-framerate"),
        "a still has no framerate: {:?}",
        cmd.input
    );
}

#[test]
fn a_snapshot_grabs_the_same_region_a_recording_would() {
    let cmd = backend_with_display(":0").grab(&GrabRequest {
        framerate: None,
        ..request()
    });
    assert_eq!(value_of(&cmd.input, "-grab_x"), Some("100"));
    assert_eq!(value_of(&cmd.input, "-grab_y"), Some("200"));
    assert_eq!(value_of(&cmd.input, "-video_size"), Some("640x480"));
    assert_eq!(value_of(&cmd.input, "-i"), Some(":0"));
}
