//! Clipping is the one piece of the geometry chain testable without a display,
//! and it is the piece that protects ffmpeg from impossible input.

use glimpse::geometry::RootPixelRect;

const ROOT_W: i32 = 3440;
const ROOT_H: i32 = 1440;

fn clip(x: i32, y: i32, w: i32, h: i32) -> RootPixelRect {
    RootPixelRect { x, y, w, h }.clipped_to(ROOT_W, ROOT_H)
}

#[test]
fn a_rect_fully_on_screen_is_untouched() {
    let r = clip(100, 100, 640, 480);
    assert_eq!(
        r,
        RootPixelRect {
            x: 100,
            y: 100,
            w: 640,
            h: 480
        }
    );
}

#[test]
fn dragging_off_the_left_edge_shrinks_rather_than_shifts() {
    // The visible region genuinely got smaller; moving the rect right instead
    // would capture pixels the user cannot see behind the frame.
    let r = clip(-200, 50, 640, 480);
    assert_eq!(
        r,
        RootPixelRect {
            x: 0,
            y: 50,
            w: 440,
            h: 480
        }
    );
}

#[test]
fn dragging_off_the_bottom_right_truncates_to_the_root() {
    let r = clip(ROOT_W - 100, ROOT_H - 50, 640, 480);
    assert_eq!(
        r,
        RootPixelRect {
            x: ROOT_W - 100,
            y: ROOT_H - 50,
            w: 100,
            h: 50
        }
    );
}

#[test]
fn a_rect_entirely_off_screen_is_refused_not_clamped_to_one_pixel() {
    let r = clip(9000, 9000, 640, 480);
    assert!(!r.is_capturable(), "got {r:?}");
}

#[test]
fn video_size_matches_the_x11grab_argument_form() {
    assert_eq!(clip(0, 0, 1280, 720).video_size(), "1280x720");
}

#[test]
fn a_zero_area_rect_is_never_capturable() {
    assert!(!RootPixelRect {
        x: 0,
        y: 0,
        w: 0,
        h: 1080
    }
    .is_capturable());
    assert!(!RootPixelRect {
        x: 0,
        y: 0,
        w: 1920,
        h: 0
    }
    .is_capturable());
}
