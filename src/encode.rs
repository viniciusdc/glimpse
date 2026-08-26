//! GIF encoding: the two-pass palette pipeline, and an atomic commit.
//!
//! **Every flag here is derived from ffmpeg's own documentation**
//! (`ffmpeg -h filter=palettegen`, `-h filter=paletteuse`,
//! <https://ffmpeg.org/ffmpeg-filters.html>), not from another project's source
//! — a licensing requirement, not a style preference (ADR 0003).
//!
//! ## Why the defaults
//!
//! `palettegen` and `paletteuse` both offer options that folklore says help with
//! screencasts. They were measured on a real 3-second capture rather than
//! assumed, and they do not:
//!
//! | option | output | vs default |
//! |---|---|---|
//! | `stats_mode=full` (default) | 1,471,070 B | — |
//! | `stats_mode=diff` | 1,453,758 B | −1.2%, inside noise |
//! | `stats_mode=single` | 2,170,981 B | **+48% worse** |
//! | `paletteuse=diff_mode=rectangle` | 1,471,577 B | marginally *larger* |
//!
//! So this uses the defaults. A flag that cannot show a benefit does not go in.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// What a recording is turned into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Two-pass palette GIF. Universally embeddable, large.
    #[default]
    Gif,
    /// H.264 in MP4. Smaller, but not an image — it will not autoplay inline
    /// everywhere a GIF does.
    ///
    /// How much smaller depends entirely on motion, and the folklore figure of
    /// "an order of magnitude" did not survive measurement: on a mostly-static
    /// 4-second desktop capture it was **1.5x** (27,974 B GIF vs 18,248 B MP4),
    /// because a palette compresses static content well. The gap widens with
    /// motion; do not promise a number.
    Mp4,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Gif => "gif",
            OutputFormat::Mp4 => "mp4",
        }
    }

    /// The ffmpeg muxer name, stated explicitly because output is staged under a
    /// `.part` suffix that ffmpeg cannot infer a format from.
    pub fn muxer(self) -> &'static str {
        match self {
            OutputFormat::Gif => "gif",
            OutputFormat::Mp4 => "mp4",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Gif => "GIF",
            OutputFormat::Mp4 => "MP4",
        }
    }

    pub fn all() -> [OutputFormat; 2] {
        [OutputFormat::Gif, OutputFormat::Mp4]
    }
}

/// H.264 arguments.
///
/// From ffmpeg's own documentation (`ffmpeg -h encoder=libx264`,
/// <https://ffmpeg.org/ffmpeg-formats.html>), not from another project (ADR 0003).
///
/// The crop filter is not optional. **H.264 with `yuv420p` requires even
/// dimensions**, and a framing window produces odd ones constantly — the first
/// real capture this project made was 754x437. Without it ffmpeg fails with
/// "Error while opening encoder".
///
/// Cropping is chosen over the alternatives deliberately. `pad` adds a visible
/// black line; `scale` resamples the entire frame, which blurs text and is the
/// worst possible outcome for a screen recording. Cropping drops at most one row
/// or column and leaves every remaining pixel untouched.
pub fn mp4_args(source: &Path, output: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
        "-vf".into(),
        "crop=trunc(iw/2)*2:trunc(ih/2)*2".into(),
        "-c:v".into(),
        "libx264".into(),
        // yuv420p rather than a higher-fidelity pixel format: it is the one every
        // player and browser decodes. Correctness of playback beats chroma here.
        "-pix_fmt".into(),
        "yuv420p".into(),
        // Moves the index to the front so the file plays before it has fully
        // downloaded — the normal case for something you share.
        "-movflags".into(),
        "+faststart".into(),
        "-f".into(),
        "mp4".into(),
        output.to_string_lossy().into_owned(),
    ]
}

/// Pass 1: analyse the whole clip and write an optimal palette.
///
/// The palette is a single PNG. `-y` because the path is ours and freshly made.
pub fn palette_args(source: &Path, palette: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
        "-vf".into(),
        "palettegen".into(),
        palette.to_string_lossy().into_owned(),
    ]
}

/// Pass 2: map the clip onto that palette.
///
/// `-lavfi` rather than `-vf` because `paletteuse` takes two inputs — the video
/// and the palette — which a simple filter chain cannot express.
pub fn encode_args(source: &Path, palette: &Path, output: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
        "-i".into(),
        palette.to_string_lossy().into_owned(),
        "-lavfi".into(),
        "paletteuse".into(),
        // Stated explicitly rather than inferred from the extension: the output
        // is staged with a `.part` suffix for the atomic commit, and ffmpeg
        // cannot guess a muxer from that.
        "-f".into(),
        "gif".into(),
        output.to_string_lossy().into_owned(),
    ]
}

/// Pick a path that does not overwrite anything.
///
/// **Collision policy: never replace, never fail — disambiguate.** Silently
/// replacing destroys a file the user may still want; failing loses a recording
/// they just made because of a name clash. `glimpse.gif` becomes `glimpse-1.gif`,
/// then `glimpse-2.gif`.
///
/// `exists` is injected so the policy is testable without touching a filesystem.
pub fn free_destination(desired: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if !exists(desired) {
        return desired.to_path_buf();
    }
    let dir = desired.parent().unwrap_or_else(|| Path::new("."));
    let stem = desired
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = desired
        .extension()
        .map(|e| e.to_string_lossy().into_owned());

    for n in 1u32.. {
        let name = match &ext {
            Some(e) => format!("{stem}-{n}.{e}"),
            None => format!("{stem}-{n}"),
        };
        let candidate = dir.join(name);
        if !exists(&candidate) {
            return candidate;
        }
    }
    unreachable!("u32 exhausted while disambiguating a filename")
}

/// Encode `source` to a GIF at (or beside) `destination`.
///
/// The commit is atomic and same-filesystem: the GIF is written to a temporary
/// name **in the destination's own directory**, then renamed. Staging in a temp
/// directory instead would make the rename cross-filesystem, which falls back to
/// a copy and is no longer atomic — a reader could observe a half-written GIF.
///
/// On any failure the partial output is removed; `source` is never touched, so a
/// failed encode costs nothing but time (ADR 0002).
pub fn encode(source: &Path, destination: &Path, format: OutputFormat) -> Result<PathBuf> {
    if !source.exists() {
        return Err(anyhow!("no recording at {}", source.display()));
    }
    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let final_path = free_destination(destination, |p| p.exists());
    let staged = dir.join(format!(
        ".glimpse-{}-{}.{}.part",
        std::process::id(),
        final_path.file_stem().unwrap_or_default().to_string_lossy(),
        format.extension()
    ));
    let palette = dir.join(format!(".glimpse-{}-palette.png", std::process::id()));

    let result = (|| -> Result<()> {
        match format {
            OutputFormat::Gif => {
                run(&palette_args(source, &palette)).context("generating the palette")?;
                run(&encode_args(source, &palette, &staged)).context("encoding the GIF")?;
            }
            OutputFormat::Mp4 => {
                run(&mp4_args(source, &staged)).context("encoding the MP4")?;
            }
        }
        Ok(())
    })();

    let _ = std::fs::remove_file(&palette);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }

    // The commit. Only now does the destination exist, and it exists whole.
    std::fs::rename(&staged, &final_path).with_context(|| {
        format!(
            "committing {} -> {}",
            staged.display(),
            final_path.display()
        )
    })?;
    Ok(final_path)
}

fn run(args: &[String]) -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(args)
        .output()
        .context("spawning ffmpeg — is it installed?")?;
    if out.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&out.stderr);
    Err(anyhow!(
        "ffmpeg exited with {}: {}",
        out.status,
        detail.lines().last().unwrap_or("no stderr")
    ))
}
