//! Output format enums: OutputFormat + ImageOutFormat (extension mapping).

use super::*;

/// How to render matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human/LLM-readable with headers (default).
    #[default]
    Text,
    /// One JSON object per emitted unit, for machine consumption.
    Json,
}

/// The four image formats the Claude API accepts - the only `image --out <file.ext>` conversion
/// targets, and the only formats a transcript's inline images are ever stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOutFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageOutFormat {
    /// The output format implied by an `--out` path EXTENSION (case-insensitive), or `None` when
    /// the extension isn't one of the four image types (⇒ the path is treated as a directory).
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(ImageOutFormat::Png),
            "jpg" | "jpeg" => Some(ImageOutFormat::Jpeg),
            "gif" => Some(ImageOutFormat::Gif),
            "webp" => Some(ImageOutFormat::Webp),
            _ => None,
        }
    }

    /// The lower-case file extension for this format.
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            ImageOutFormat::Png => "png",
            ImageOutFormat::Jpeg => "jpg",
            ImageOutFormat::Gif => "gif",
            ImageOutFormat::Webp => "webp",
        }
    }

    /// The canonical `image/*` media type.
    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            ImageOutFormat::Png => "image/png",
            ImageOutFormat::Jpeg => "image/jpeg",
            ImageOutFormat::Gif => "image/gif",
            ImageOutFormat::Webp => "image/webp",
        }
    }
}
