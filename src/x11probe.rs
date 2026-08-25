//! Direct X11 queries. GTK4 deliberately refuses to tell an application where it
//! is on screen (ADR 0001), so we ask the server ourselves.

use anyhow::Result;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as ShapeConnectionExt};
use x11rb::protocol::xproto::{ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

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
    /// This is the ONLY trustworthy way to verify an input region. Pointer-position
    /// queries cannot do it: `XQueryPointer`'s child field is geometric and blind to
    /// input shape — proven by a control probe during the spike (ADR 0000).
    pub fn input_shape(&self, xid: u32) -> Result<Vec<(i16, i16, u16, u16)>> {
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
