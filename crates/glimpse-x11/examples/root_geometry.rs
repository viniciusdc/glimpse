//! Query X directly, with no GTK window involved.
//!
//! Demonstrates the half of Glimpse that GTK4 cannot provide: where a window
//! actually is, and what input shape the server actually holds for it.
//!
//! ```sh
//! cargo run -p glimpse-x11 --example root_geometry              # root size only
//! cargo run -p glimpse-x11 --example root_geometry -- 0x9a00004 # plus that window
//! ```
//!
//! Handy for confirming a suspicion about click-through: if `input shape` comes
//! back empty, the region never took effect, whatever the pointer seems to do.

use anyhow::Result;
use glimpse_x11::x11probe::X11Probe;

fn main() -> Result<()> {
    let probe = X11Probe::new()?;
    let (w, h) = probe.root_size()?;
    println!("root: {w}x{h}");

    let Some(arg) = std::env::args().nth(1) else {
        println!("(pass a window id — e.g. from `xdotool getactivewindow` — for more)");
        return Ok(());
    };

    let xid = arg
        .strip_prefix("0x")
        .map(|h| u32::from_str_radix(h, 16))
        .unwrap_or_else(|| arg.parse())?;

    let (x, y) = probe.surface_origin(xid)?;
    println!("window 0x{xid:x} origin: {x},{y}");

    match probe.input_shape(xid)? {
        s if s.is_empty() => println!("input shape: none — the whole window takes clicks"),
        s => {
            println!("input shape: {} band(s)", s.len());
            for (bx, by, bw, bh) in s {
                println!("  {bw}x{bh}+{bx}+{by}");
            }
        }
    }
    Ok(())
}
