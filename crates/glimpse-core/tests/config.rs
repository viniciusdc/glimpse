//! Settings persistence. No display and no GTK involved — this is plain data.

use glimpse_core::config::{Config, Mode, Theme};
use glimpse_core::encode::OutputFormat;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("glimpse-cfg-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("config.toml")
}

#[test]
fn defaults_are_usable_without_a_config_file() {
    let cfg = Config::load_from(&PathBuf::from("/nonexistent/glimpse/config.toml"));
    assert_eq!(
        cfg.theme,
        Theme::System,
        "follow the desktop until told otherwise"
    );
    assert_eq!(cfg.format, OutputFormat::Gif);
    assert_eq!(cfg.framerate, 15);
    assert!(cfg.capture_mouse);
    assert!(cfg.output_dir.is_absolute(), "got {:?}", cfg.output_dir);
}

#[test]
fn settings_survive_a_round_trip() {
    let path = scratch("roundtrip");
    let cfg = Config {
        theme: Theme::Light,
        mode: Mode::Snapshot,
        format: OutputFormat::Mp4,
        output_dir: PathBuf::from("/home/u/Recordings"),
        framerate: 24,
        capture_mouse: false,
    };
    cfg.save_to(&path).unwrap();
    assert_eq!(Config::load_from(&path), cfg);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_corrupt_config_falls_back_instead_of_refusing_to_start() {
    // Losing a preference is annoying. Refusing to launch a screen recorder
    // because a settings file has a stray bracket in it is worse.
    let path = scratch("corrupt");
    std::fs::write(&path, "this is not = [valid toml").unwrap();
    assert_eq!(Config::load_from(&path), Config::default());
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_partial_config_keeps_the_defaults_for_everything_else() {
    // Hand-edited files are normal; an unknown-shaped file should not reset the
    // settings the user did write.
    let path = scratch("partial");
    std::fs::write(&path, "theme = \"dark\"\n").unwrap();
    let cfg = Config::load_from(&path);
    assert_eq!(cfg.theme, Theme::Dark);
    assert_eq!(cfg.format, OutputFormat::Gif);
    assert_eq!(cfg.framerate, 15);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn the_destination_is_the_chosen_folder_plus_a_name_glimpse_picks() {
    // Only the directory is the user's choice — the filename stays ours so the
    // collision handling in encode.rs keeps working.
    let cfg = Config {
        output_dir: PathBuf::from("/home/u/Videos"),
        format: OutputFormat::Mp4,
        ..Config::default()
    };
    assert_eq!(
        cfg.destination(),
        PathBuf::from("/home/u/Videos/glimpse.mp4")
    );

    let gif = Config {
        format: OutputFormat::Gif,
        ..cfg
    };
    assert_eq!(
        gif.destination(),
        PathBuf::from("/home/u/Videos/glimpse.gif")
    );
}

#[test]
fn themes_round_trip_through_their_identifiers() {
    for t in Theme::all() {
        assert_eq!(Theme::from_id(t.id()), Some(t));
    }
    assert_eq!(Theme::from_id("solarized"), None);
}

#[test]
fn a_snapshot_is_always_png_whatever_the_recording_format_is() {
    // A still frame is an image; the recording format has nothing to say about it.
    let cfg = Config {
        output_dir: PathBuf::from("/home/u/Videos"),
        format: OutputFormat::Mp4,
        ..Config::default()
    };
    assert_eq!(
        cfg.snapshot_destination(),
        PathBuf::from("/home/u/Videos/glimpse.png")
    );
}

#[test]
fn the_button_remembers_what_it_last_did() {
    let path = scratch("mode");
    let cfg = Config {
        mode: Mode::Snapshot,
        ..Config::default()
    };
    cfg.save_to(&path).unwrap();
    assert_eq!(Config::load_from(&path).mode, Mode::Snapshot);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn modes_round_trip_through_their_identifiers() {
    for m in Mode::all() {
        assert_eq!(Mode::from_id(m.id()), Some(m));
    }
    assert_eq!(Mode::from_id("burst"), None);
}
