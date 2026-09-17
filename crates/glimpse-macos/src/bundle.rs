//! Point GTK at the bundle's own data files, when there is a bundle.
//!
//! `otool` sees dylibs. It does not see the files a GTK application needs that
//! are not libraries: the compiled GSettings schemas, the icon theme the header's
//! symbolic icons come from, the pixbuf loader cache. Those are found through
//! `XDG_DATA_DIRS` and glib's compiled-in default directory, and glib's default
//! is the prefix it was *built* with — `/opt/homebrew/share` for a Homebrew GTK.
//!
//! So a bundle that carries all 39 dylibs and none of this runs perfectly on the
//! machine that built it and cannot find its icons anywhere else. That is the
//! shape of failure [ADR 0013](../../docs/adr/0013-macos-ships-an-app-bundle.md)
//! was written to avoid, arriving through the door the dylib work does not cover.
//!
//! ## Why this is in the app and not the build script
//!
//! `scripts/bundle-macos.sh` copies the files in. Nothing reads them unless the
//! process says where they are, and the only thing that knows where the bundle
//! is at runtime is the process itself — a bundle can be dragged anywhere, so
//! the path cannot be baked in at build time.
//!
//! ## It must run before GTK
//!
//! GLib reads these once, early. Setting them after `gtk::Application` exists is
//! setting them too late, and the failure is silent: the variables are correct
//! and nothing consulted them.

use std::path::PathBuf;

/// Where a bundle keeps the things that are not libraries.
///
/// `None` when the binary is not inside a `.app`, which is the normal case for
/// `cargo run` and for anyone building from source. Nothing is set then: a
/// development build finds Homebrew's copies through the ordinary search path,
/// and overriding that would break the common case to fix the rare one.
fn resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // .../Glimpse.app/Contents/MacOS/glimpse -> .../Glimpse.app/Contents
    let contents = exe.parent()?.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let resources = contents.join("Resources");
    resources.is_dir().then_some(resources)
}

/// Tell GLib and GTK where the bundle's data lives.
///
/// Call before anything touches GTK. Returns what it set, for the diagnostics
/// report — a bundle whose resources were not found should be able to say so
/// rather than being discovered later by its icons being blank.
pub fn configure() -> Option<String> {
    let resources = resources()?;
    let share = resources.join("share");
    let schemas = share.join("glib-2.0/schemas");

    // GSETTINGS_SCHEMA_DIR is consulted in addition to the compiled-in default,
    // which is why a missing schema only bites on a machine that does not also
    // happen to have Homebrew. Setting it is what makes the bundle's own copy
    // the one that is found.
    if schemas.is_dir() {
        std::env::set_var("GSETTINGS_SCHEMA_DIR", &schemas);
    }

    // Prepended, not replaced. Anything already there belongs to the user's
    // session and taking it away would be a bigger change than this needs to
    // make; the bundle only has to be looked at first.
    if share.is_dir() {
        let mut dirs = share.as_os_str().to_os_string();
        if let Ok(existing) = std::env::var("XDG_DATA_DIRS") {
            if !existing.is_empty() {
                dirs.push(":");
                dirs.push(existing);
            }
        }
        std::env::set_var("XDG_DATA_DIRS", dirs);
    }

    Some(format!("resources    : {}\n", resources.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_binary_is_not_a_bundle() {
        // The test binary is not inside a `.app`, so this is the development
        // case: nothing is set and Homebrew's own search path is left alone.
        // Getting this wrong would break `cargo run` for everyone in order to
        // fix a bundle nobody is running yet.
        assert!(resources().is_none());
        assert!(configure().is_none());
        assert!(std::env::var("GSETTINGS_SCHEMA_DIR").is_err());
    }
}
