//! The macOS capture backend: how a rectangle becomes an `avfoundation`
//! invocation.
//!
//! **Every flag here is derived from ffmpeg's own documentation**
//! (`ffmpeg -h demuxer=avfoundation`, <https://ffmpeg.org/ffmpeg-devices.html>),
//! not from another project's source. That is a licensing requirement, not a
//! style preference — see ADR 0003.
//!
//! This is the other half of the seam ADR 0010 describes. `glimpse-core` owns the
//! ffmpeg child and the output encoding and cannot name a backend; everything
//! specific to macOS stops here.
//!
//! ## How this differs from x11grab, and why the seam is shaped as it is
//!
//! `x11grab` takes the region on the input with `-grab_x`/`-grab_y`.
//! `avfoundation` has no such option: it captures a whole display, and the region
//! comes from a `crop` filter afterwards. That asymmetry is the entire reason
//! [`GrabCommand`] carries an optional filter rather than being a flat argument
//! list.

use anyhow::{anyhow, Context, Result};
use glimpse_core::capture::{GrabCommand, GrabRequest};

/// The macOS capture backend, holding the avfoundation device it will record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvfCapture {
    /// The screen device's index in avfoundation's own numbering.
    ///
    /// Discovered, never assumed. The screen devices are numbered after the
    /// cameras in one shared list, so the index moves with the hardware — on the
    /// machine this was written against, `Capture screen 0` is device **2**,
    /// behind two built-in cameras.
    device: String,
}

impl AvfCapture {
    /// Use a known device index.
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
        }
    }

    /// Ask ffmpeg which device is the screen.
    ///
    /// Listing devices is an error path for ffmpeg — it prints the list and then
    /// exits non-zero with "Input/output error", because `-i ""` names no real
    /// input. So the status is deliberately ignored and the output is parsed
    /// instead; treating a non-zero exit as failure here would reject a perfectly
    /// good listing.
    pub fn discover() -> Result<Self> {
        let out = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-f",
                "avfoundation",
                "-list_devices",
                "true",
                "-i",
                "",
            ])
            .output()
            .context("spawning ffmpeg — is it installed?")?;

        // avfoundation prints its device list to stderr.
        let listing = String::from_utf8_lossy(&out.stderr);
        let index = screen_device_index(&listing).ok_or_else(|| {
            anyhow!(
                "ffmpeg listed no screen capture device.\n\
                 On macOS this usually means Screen Recording permission has not been \
                 granted to the program running Glimpse — note that macOS grants it to \
                 the terminal, not to the binary, which reads like a bug in the app.\n\
                 Devices reported:\n{listing}"
            )
        })?;
        Ok(Self::new(index))
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    /// Build the grab command for `request`.
    ///
    /// Options, each from `ffmpeg -h demuxer=avfoundation`:
    ///   `-capture_cursor`  draw the pointer
    ///   `-framerate`       capture rate
    ///   `-pix_fmt`         pixel format requested from the device
    ///   `-i <index>:none`  video device index, no audio
    pub fn grab(&self, request: &GrabRequest) -> GrabCommand {
        let s = |v: &str| v.to_string();
        let mut input = vec![s("-f"), s("avfoundation")];

        // Emitted in both directions rather than left to the device default, for
        // the same reason x11grab states `-draw_mouse`: a setting the user can
        // toggle should not depend on what the backend happens to prefer.
        //
        // **MEASURED TO BE IGNORED**, and ignored in the OFF direction:
        // avfoundation never draws the pointer, whatever this says. Against a
        // static window with the pointer parked in it and a cursor provably
        // rendered there, three runs gave a noise floor of 0 and a signal of 0,
        // and the frame was an exact match for a cursor-free reference capture
        // (1.2e7 differing pixels against a with-cursor one).
        //
        // It is still emitted, because the cost is one argument and a future
        // ffmpeg that honours it would then work without a code change. What is
        // NOT acceptable is the silence: `capture_mouse` is a user-facing toggle
        // that persists to `config.toml`, and on this backend it does nothing.
        // The user sees a switch that flips and a recording that never changes.
        //
        // `GrabCommand` has no channel for "I could not honour this", so core
        // cannot know and neither can the UI. That gap now has a confirmed
        // instance behind it rather than a suspected one — see issue #1.
        input.push(s("-capture_cursor"));
        input.push(if request.capture_mouse {
            s("1")
        } else {
            s("0")
        });

        // Omitted entirely for a single frame; a snapshot has no capture rate.
        if let Some(fps) = request.framerate {
            input.push(s("-framerate"));
            input.push(fps.to_string());
        }

        // Requested on the INPUT, which is what makes the intermediate
        // conversion-free. Measured: avfoundation offers uyvy422, yuyv422, nv12,
        // 0rgb and bgr0, and asking for bgr0 yields `ffv1 ... bgr0` out of
        // ffprobe. So ADR 0002's refusal to call the intermediate lossless
        // without evidence is satisfied on macOS by the same argument as on X11.
        input.push(s("-pix_fmt"));
        input.push(s(PIX_FMT));

        input.push(s("-i"));
        input.push(format!("{}:none", self.device));

        GrabCommand {
            rect: request.rect,
            input,
            filter: Some(crop_filter(request)),
            pix_fmt: Some(s(PIX_FMT)),
        }
    }
}

/// The one pixel format worth asking for, and the reason it is a constant: the
/// input request and the output declaration have to agree, or the "nothing is
/// converted" claim quietly stops being true.
const PIX_FMT: &str = "bgr0";

/// The `crop` filter for a request.
///
/// **No coordinate flip happens here, deliberately.** `ScreenPixelRect` is
/// documented as global device pixels with a top-left origin and y increasing
/// downward, and ffmpeg's `crop` counts from the top-left with y downward too, so
/// the values pass straight through.
///
/// The flip that macOS does need belongs to whatever produces the rectangle:
/// AppKit puts (0,0) at the bottom-left of the primary screen. Doing it here as
/// well would apply it twice, and a rectangle that is the right size on the wrong
/// pixels looks entirely plausible in a log line — which is the failure ADR 0000
/// exists to record.
pub fn crop_filter(request: &GrabRequest) -> String {
    let r = request.rect;
    format!("crop={}:{}:{}:{}", r.w, r.h, r.x, r.y)
}

/// The device index of the first screen in an `-list_devices` listing.
///
/// Split out from [`AvfCapture::discover`] so the parsing is testable without
/// ffmpeg and without a screen. The listing looks like:
///
/// ```text
/// [AVFoundation indev @ 0x…] AVFoundation video devices:
/// [AVFoundation indev @ 0x…] [0] Câmera do MacBook Pro
/// [AVFoundation indev @ 0x…] [2] Capture screen 0
/// [AVFoundation indev @ 0x…] AVFoundation audio devices:
/// [AVFoundation indev @ 0x…] [0] MOMENTUM 4
/// ```
///
/// Two traps live in that shape. The numbering restarts for audio devices, so a
/// match has to stop at the audio header or it can return an audio index. And
/// device names are localised — the cameras above are in Portuguese — so only the
/// English `Capture screen` substring, which ffmpeg generates itself rather than
/// taking from the system, is safe to match on.
pub fn screen_device_index(listing: &str) -> Option<String> {
    for line in listing.lines() {
        if line.contains("AVFoundation audio devices") {
            break;
        }
        if !line.contains("Capture screen") {
            continue;
        }
        let start = line.find('[')?;
        let rest = &line[start..];
        // The line carries at least two bracketed groups: the log prefix
        // `[AVFoundation indev @ 0x…]` and the device `[2]`. Take the last one
        // that parses as a number.
        let mut found = None;
        for (i, c) in rest.char_indices() {
            if c != '[' {
                continue;
            }
            let Some(end) = rest[i..].find(']') else {
                continue;
            };
            let inner = &rest[i + 1..i + end];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                found = Some(inner.to_string());
            }
        }
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use glimpse_core::geometry::ScreenPixelRect;

    /// Real output from `ffmpeg -f avfoundation -list_devices true -i ""`, kept
    /// verbatim including the localised camera names, because those are exactly
    /// what a naive parser would trip over.
    const LISTING: &str = "\
[AVFoundation indev @ 0xac6c08140] AVFoundation video devices:
[AVFoundation indev @ 0xac6c08140] [0] Câmera do MacBook Pro
[AVFoundation indev @ 0xac6c08140] [1] Câmera para Visualização da Mesa do MacBook Pro
[AVFoundation indev @ 0xac6c08140] [2] Capture screen 0
[AVFoundation indev @ 0xac6c08140] AVFoundation audio devices:
[AVFoundation indev @ 0xac6c08140] [0] MOMENTUM 4
[AVFoundation indev @ 0xac6c08140] [1] Microfone (MacBook Pro)
";

    fn request(framerate: Option<u32>) -> GrabRequest {
        GrabRequest {
            rect: ScreenPixelRect {
                x: 480,
                y: 764,
                w: 1280,
                h: 800,
            },
            framerate,
            capture_mouse: false,
        }
    }

    /// The screen sits behind the cameras, so a parser that takes the first
    /// device, or assumes 0, records a webcam.
    #[test]
    fn the_screen_index_is_found_behind_the_cameras() {
        assert_eq!(screen_device_index(LISTING).as_deref(), Some("2"));
    }

    /// Audio numbering restarts from 0. A search that runs past the audio header
    /// can return an audio index that happens to sit on a matching line.
    #[test]
    fn parsing_stops_before_the_audio_devices() {
        let with_audio_screen =
            format!("{LISTING}[AVFoundation indev @ 0x1] [7] Capture screen 9\n");
        assert_eq!(
            screen_device_index(&with_audio_screen).as_deref(),
            Some("2"),
            "the video section's answer must win"
        );
    }

    #[test]
    fn a_listing_with_no_screen_yields_nothing() {
        let cameras_only = "\
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] Câmera do MacBook Pro
";
        assert_eq!(screen_device_index(cameras_only), None);
    }

    /// The rect is already top-left device pixels and `crop` counts the same way,
    /// so the numbers pass through untouched. If this ever starts subtracting
    /// from a screen height, the flip is being applied twice.
    #[test]
    fn the_crop_uses_the_rect_verbatim_with_no_flip() {
        assert_eq!(crop_filter(&request(Some(15))), "crop=1280:800:480:764");
    }

    /// avfoundation cannot express the region on the input, so unlike x11grab a
    /// filter is mandatory. A `None` here would record the whole display.
    #[test]
    fn a_crop_filter_is_always_produced() {
        let cmd = AvfCapture::new("2").grab(&request(Some(15)));
        assert_eq!(cmd.filter.as_deref(), Some("crop=1280:800:480:764"));
        assert!(
            !cmd.input.iter().any(|a| a == "-grab_x"),
            "avfoundation has no -grab_x; emitting one would be an x11grab-ism"
        );
    }

    #[test]
    fn the_device_index_reaches_the_input_specifier() {
        let cmd = AvfCapture::new("2").grab(&request(Some(15)));
        let i = cmd.input.iter().position(|a| a == "-i").expect("input");
        assert_eq!(cmd.input[i + 1], "2:none", "video device, no audio");
    }

    /// Requested on the input AND declared on the output. If those disagree,
    /// ffmpeg converts and the intermediate stops being lossless by construction.
    #[test]
    fn the_pixel_format_is_requested_and_declared_consistently() {
        let cmd = AvfCapture::new("2").grab(&request(Some(15)));
        let i = cmd
            .input
            .iter()
            .position(|a| a == "-pix_fmt")
            .expect("pix_fmt");
        assert_eq!(cmd.input[i + 1], "bgr0");
        assert_eq!(cmd.pix_fmt.as_deref(), Some("bgr0"));
    }

    #[test]
    fn a_snapshot_sets_no_capture_rate() {
        let cmd = AvfCapture::new("2").grab(&request(None));
        assert!(
            !cmd.input.iter().any(|a| a == "-framerate"),
            "{:?}",
            cmd.input
        );
    }

    #[test]
    fn the_mouse_flag_is_explicit_either_way() {
        let backend = AvfCapture::new("2");
        let off = backend.grab(&request(Some(15)));
        let i = off
            .input
            .iter()
            .position(|a| a == "-capture_cursor")
            .unwrap();
        assert_eq!(off.input[i + 1], "0");

        let on = backend.grab(&GrabRequest {
            capture_mouse: true,
            ..request(Some(15))
        });
        let i = on
            .input
            .iter()
            .position(|a| a == "-capture_cursor")
            .unwrap();
        assert_eq!(on.input[i + 1], "1");
    }
}
