//! The X11 platform boundary: everything that talks to the X server, plus the
//! one GTK call that reaches through to a raw window id.
//!
//! GTK4 refuses to tell an application where it is on screen (ADR 0001), so we
//! ask the server ourselves. Keeping those calls here means `geometry` depends
//! on the platform layer rather than on the UI layer.

use anyhow::{anyhow, Result};
use gdk4_x11::{X11Display, X11Surface};
use gtk::prelude::*;
use gtk4 as gtk;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as ShapeConnectionExt};
use x11rb::protocol::xproto::{ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

/// One band of an input-shape region, in surface coordinates.
pub type ShapeBand = (i16, i16, u16, u16);

/// Fail unless GTK is actually driving an X11 display.
///
/// Connecting to an X server is **not** sufficient evidence: under Wayland,
/// XWayland usually has `DISPLAY` set and accepts connections, so an X11 probe
/// succeeds while GTK quietly selects its Wayland backend. Without this check the
/// application presents a framing window and only discovers it cannot record when
/// the user asks for a rectangle.
pub fn require_x11_display() -> Result<()> {
    let display = gtk::gdk::Display::default()
        .ok_or_else(|| anyhow!("no GDK display — is a display server running?"))?;
    if display.downcast_ref::<X11Display>().is_none() {
        return Err(anyhow!(
            "GTK is using the {} backend, not X11.\n\
             Glimpse v0.1 is X11-only by design, not by omission (ADR 0002): a compositor\n\
             that mediates screen selection cannot host a window that picks its own capture\n\
             rectangle. Under a Wayland session, GDK_BACKEND=x11 will run it through XWayland,\n\
             which is untested and unsupported.",
            display.type_().name()
        ));
    }
    Ok(())
}

/// Raw X11 window id for a GTK window.
///
/// Uses `GdkX11Surface::xid`, deprecated since GTK 4.18 with no documented
/// replacement (ADR 0001). This function is the single choke point, so the day it
/// breaks, one function changes — though note that if GTK removes *access* rather
/// than renaming it, the fallback is a different window-ownership model, which is
/// a larger change than one edit here.
pub fn window_xid(window: &gtk::Window) -> Result<u32> {
    let surface = window
        .surface()
        .ok_or_else(|| anyhow!("window has no surface — not realized yet"))?;
    let x11 = surface
        .downcast_ref::<X11Surface>()
        .ok_or_else(|| anyhow!("not an X11 surface — Glimpse v0.1 is X11-only (ADR 0002)"))?;
    Ok(x11.xid() as u32)
}

/// Does an input-shape region cover this point? Points the region covers take
/// clicks; points it does not are click-through.
pub fn shape_covers(bands: &[ShapeBand], x: i32, y: i32) -> bool {
    bands.iter().any(|&(bx, by, bw, bh)| {
        x >= bx as i32 && x < bx as i32 + bw as i32 && y >= by as i32 && y < by as i32 + bh as i32
    })
}

pub struct X11Probe {
    conn: RustConnection,
    /// The root of OUR screen — never an assumed default root.
    root: Window,
}

impl X11Probe {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        Ok(Self { conn, root })
    }

    /// Absolute root-pixel position of a window's origin.
    pub fn surface_origin(&self, xid: u32) -> Result<(i32, i32)> {
        let t = self
            .conn
            .translate_coordinates(xid, self.root, 0, 0)?
            .reply()?;
        Ok((t.dst_x as i32, t.dst_y as i32))
    }

    /// Root dimensions, used to clip the capture rect to something real.
    pub fn root_size(&self) -> Result<(i32, i32)> {
        let g = self.conn.get_geometry(self.root)?.reply()?;
        Ok((g.width as i32, g.height as i32))
    }

    /// Read back the input shape the server actually holds.
    ///
    /// This is the only trustworthy way to verify an input region. Pointer
    /// queries cannot do it: `XQueryPointer`'s child field is geometric and blind
    /// to input shape — proven by a control probe during the spike (ADR 0000).
    ///
    /// Note that a window with no shape set does not necessarily report zero
    /// bands; check the geometry with [`shape_covers`] rather than counting.
    pub fn input_shape(&self, xid: u32) -> Result<Vec<ShapeBand>> {
        let r = self
            .conn
            .shape_get_rectangles(xid, shape::SK::INPUT)?
            .reply()?;
        Ok(r.rectangles
            .iter()
            .map(|k| (k.x, k.y, k.width, k.height))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::shape_covers;

    /// A window-minus-hole region: four bands around a hole at (10,10,80,80)
    /// inside a 100x100 window.
    const FRAME: &[(i16, i16, u16, u16)] = &[
        (0, 0, 100, 10),
        (0, 10, 10, 80),
        (90, 10, 10, 80),
        (0, 90, 100, 10),
    ];

    #[test]
    fn the_hole_is_click_through() {
        assert!(
            !shape_covers(FRAME, 50, 50),
            "hole centre must not take clicks"
        );
    }

    #[test]
    fn the_border_takes_clicks() {
        assert!(shape_covers(FRAME, 50, 5), "top band");
        assert!(shape_covers(FRAME, 5, 50), "left band");
        assert!(shape_covers(FRAME, 95, 50), "right band");
        assert!(shape_covers(FRAME, 50, 95), "bottom band");
    }

    #[test]
    fn band_edges_are_half_open() {
        // The hole starts at x=10, so x=9 is border and x=10 is hole.
        assert!(shape_covers(FRAME, 9, 50));
        assert!(!shape_covers(FRAME, 10, 50));
    }

    #[test]
    fn an_empty_region_covers_nothing() {
        assert!(!shape_covers(&[], 0, 0));
    }
}
