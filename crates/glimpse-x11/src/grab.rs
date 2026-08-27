//! The X11 capture backend: how a rectangle becomes an `x11grab` invocation.
//!
//! **Every flag here is derived from ffmpeg's own documentation**
//! (`ffmpeg -h demuxer=x11grab`, <https://ffmpeg.org/ffmpeg-devices.html>), not
//! from another project's source. That is a licensing requirement, not a style
//! preference — see ADR 0003.
//!
//! This is one half of the seam ADR 0010 describes: `glimpse-core` owns the
//! ffmpeg child and the output encoding, and knows nothing about how the region
//! is expressed. Everything specific to X11 stops here.

use anyhow::{Context, Result};
use glimpse_core::capture::{GrabCommand, GrabRequest};

/// The X11 capture backend, holding the display it was told to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Capture {
    /// The X display, taken from the same environment the geometry came from —
    /// never independently guessed, or ffmpeg could record a different screen
    /// than the one the rectangle was computed against.
    display: String,
}

impl X11Capture {
    /// Read the display from the environment. Returns an error rather than
    /// falling back to `:0`.
    pub fn from_env() -> Result<Self> {
        let display = std::env::var("DISPLAY").context("DISPLAY is not set; cannot record")?;
        Ok(Self { display })
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    /// Build the grab command for `request`.
    ///
    /// Options, each from `ffmpeg -h demuxer=x11grab`:
    ///   `-video_size`  frame size
    ///   `-framerate`   capture rate
    ///   `-draw_mouse`  0 or 1, draw the pointer
    ///   `-grab_x` / `-grab_y`   region origin
    ///
    /// The documented `-grab_x`/`-grab_y` options are used rather than encoding
    /// the origin into the input URL, because they are explicit and cannot be
    /// mangled by a display name that already contains punctuation.
    ///
    /// No filter is produced. x11grab grabs only the region asked for, so unlike
    /// `avfoundation` there is nothing to crop after the fact.
    pub fn grab(&self, request: &GrabRequest) -> GrabCommand {
        let s = |v: &str| v.to_string();
        let mut input = vec![s("-f"), s("x11grab")];

        // Omitted entirely for a single frame: a snapshot has no capture rate,
        // and x11grab would otherwise be told to run at one.
        if let Some(fps) = request.framerate {
            input.push(s("-framerate"));
            input.push(fps.to_string());
        }

        input.extend([
            s("-video_size"),
            request.rect.video_size(),
            s("-draw_mouse"),
            if request.capture_mouse {
                s("1")
            } else {
                s("0")
            },
            s("-grab_x"),
            request.rect.x.to_string(),
            s("-grab_y"),
            request.rect.y.to_string(),
            s("-i"),
            self.display.clone(),
        ]);

        GrabCommand {
            rect: request.rect,
            input,
            filter: None,
            // x11grab's native output is bgr0, so stating it keeps the
            // intermediate conversion-free and therefore lossless by
            // construction rather than by assertion (ADR 0002).
            pix_fmt: Some(s("bgr0")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glimpse_core::geometry::ScreenPixelRect;

    fn backend() -> X11Capture {
        X11Capture {
            display: ":99".into(),
        }
    }

    fn request(framerate: Option<u32>) -> GrabRequest {
        GrabRequest {
            rect: ScreenPixelRect {
                x: 12,
                y: 34,
                w: 640,
                h: 480,
            },
            framerate,
            capture_mouse: false,
        }
    }

    /// The origin travels as documented options rather than inside the input URL,
    /// so a display name containing punctuation cannot mangle it.
    #[test]
    fn the_region_travels_as_grab_x_and_grab_y() {
        let cmd = backend().grab(&request(Some(30)));
        let at = |flag: &str| {
            let i = cmd.input.iter().position(|a| a == flag).expect(flag);
            cmd.input[i + 1].clone()
        };
        assert_eq!(at("-grab_x"), "12");
        assert_eq!(at("-grab_y"), "34");
        assert_eq!(at("-video_size"), "640x480");
        assert_eq!(at("-i"), ":99");
    }

    /// x11grab crops by grabbing; a filter here would mean the region was applied
    /// twice.
    #[test]
    fn x11grab_needs_no_crop_filter() {
        assert_eq!(backend().grab(&request(Some(30))).filter, None);
    }

    #[test]
    fn a_snapshot_sets_no_capture_rate() {
        let cmd = backend().grab(&request(None));
        assert!(
            !cmd.input.iter().any(|a| a == "-framerate"),
            "a single frame has no framerate"
        );
    }

    /// Stating x11grab's native format is what makes the intermediate
    /// conversion-free, which is the whole basis for calling it lossless.
    #[test]
    fn the_native_pixel_format_is_stated() {
        assert_eq!(
            backend().grab(&request(Some(30))).pix_fmt.as_deref(),
            Some("bgr0")
        );
    }

    #[test]
    fn the_mouse_flag_is_explicit_either_way() {
        let mut req = request(Some(30));
        let off = backend().grab(&req);
        let i = off.input.iter().position(|a| a == "-draw_mouse").unwrap();
        assert_eq!(off.input[i + 1], "0");

        req.capture_mouse = true;
        let on = backend().grab(&req);
        let i = on.input.iter().position(|a| a == "-draw_mouse").unwrap();
        assert_eq!(on.input[i + 1], "1");
    }
}
