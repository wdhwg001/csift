//! Media-type mapping + transcode (png/jpg/gif/webp) + extraction write.

use super::*;

/// A source `media_type` → an [`ImageOutFormat`]. `None` for a media type outside the four
/// Claude-API image types (shouldn't occur — CC only stores those).
pub(crate) fn format_of_media_type(mt: &str) -> Option<ImageOutFormat> {
    match mt {
        "image/png" => Some(ImageOutFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageOutFormat::Jpeg),
        "image/gif" => Some(ImageOutFormat::Gif),
        "image/webp" => Some(ImageOutFormat::Webp),
        _ => None,
    }
}

/// Decode `bytes` and RE-ENCODE to `target` (caller handles the same-format passthrough). An
/// animated GIF flattened to a still target yields its FIRST frame (with a warning note);
/// →jpeg is quality-90 lossy, →gif is palette-quantized, →webp is pure-Rust lossless. Returns
/// the encoded bytes plus any short human notes. A decode/encode failure is surfaced, never a
/// wrong file.
pub(crate) fn convert_image(
    bytes: &[u8],
    target: ImageOutFormat,
) -> Result<(Vec<u8>, Vec<String>)> {
    let mut notes = Vec::new();
    let src_is_gif = image::guess_format(bytes).ok() == Some(image::ImageFormat::Gif);

    // Decode to a single still image. An animated GIF → still target keeps only the first frame.
    let img: image::DynamicImage = if src_is_gif && target != ImageOutFormat::Gif {
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
            .context("decode gif")?;
        let frames = image::AnimationDecoder::into_frames(decoder)
            .collect_frames()
            .context("read gif frames")?;
        let n = frames.len();
        let total_s: f64 = frames
            .iter()
            .map(|f| {
                let (num, den) = f.delay().numer_denom_ms();
                if den == 0 {
                    0.0
                } else {
                    f64::from(num) / f64::from(den)
                }
            })
            .sum::<f64>()
            / 1000.0;
        let first = frames
            .into_iter()
            .next()
            .context("animated gif has no frames")?;
        if n > 1 {
            notes.push(format!(
                "animated GIF ({n} frames, {total_s:.1}s) — extracted the first frame only"
            ));
        }
        image::DynamicImage::ImageRgba8(first.into_buffer())
    } else {
        image::load_from_memory(bytes).context("decode source image")?
    };

    let mut out = Vec::new();
    match target {
        ImageOutFormat::Png => {
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .context("encode png")?;
        }
        ImageOutFormat::Jpeg => {
            // JPEG has no alpha → flatten to RGB; quality fixed at 90.
            let rgb = img.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut std::io::Cursor::new(&mut out),
                90,
            )
            .encode_image(&rgb)
            .context("encode jpeg")?;
            notes.push("lossy re-encode (jpeg quality 90)".to_string());
        }
        ImageOutFormat::Gif => {
            // Build a ≤256-color palette (NeuQuant) and Floyd-Steinberg dither INTO it before
            // encoding, so a photographic source doesn't band. `image`'s dither diffuses error to
            // x+1 unconditionally → it panics on a 1px-wide buffer; a sub-2px image has nothing to
            // dither anyway, so skip it there (the encoder still quantizes) and never crash.
            let mut rgba = img.to_rgba8();
            let dithered = rgba.width() >= 2 && rgba.height() >= 2;
            if dithered {
                let nq = color_quant::NeuQuant::new(10, 256, rgba.as_raw());
                image::imageops::colorops::dither(&mut rgba, &nq);
            }
            image::codecs::gif::GifEncoder::new(&mut std::io::Cursor::new(&mut out))
                .encode_frame(image::Frame::new(rgba))
                .context("encode gif")?;
            notes.push(
                if dithered {
                    "dithered to a ≤256-color palette"
                } else {
                    "quantized to a ≤256-color palette"
                }
                .to_string(),
            );
        }
        ImageOutFormat::Webp => {
            // libwebp (the `webp` crate) — proper lossy quality-90, not a lossless-only fallback.
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            out = webp::Encoder::from_rgba(rgba.as_raw(), w, h)
                .encode(90.0)
                .to_vec();
            notes.push("lossy re-encode (webp quality 90)".to_string());
        }
    }
    Ok((out, notes))
}

/// Decode + write the selected image(s) to `out_path`. The path's EXTENSION drives the format
/// (the `convert in out.jpg` idiom): a path WITH a `png`/`jpg`/`jpeg`/`gif`/`webp` extension is a
/// single-FILE target in that format (the one selected image is CONVERTED to it if it differs);
/// any other path is a DIRECTORY, each image auto-named in its SOURCE format. A decode/IO failure
/// is surfaced (never a silent or partial-wrong file).
pub(crate) fn extract(
    selected: &[&ImageRef],
    out_path: &Path,
    args: &ImageArgs,
    skipped_lines: usize,
) -> Result<()> {
    let file_target = out_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(ImageOutFormat::from_ext);

    if let Some(t) = file_target {
        // One file ⇒ exactly one image; create the parent dir (the path itself is the file).
        if selected.len() != 1 {
            bail!(
                "--out {} is a single {} file, but {} image(s) are selected — give a directory \
                 (a path with no image extension) to extract many, or narrow `--id` to one",
                out_path.display(),
                t.ext(),
                selected.len()
            );
        }
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
        }
    } else {
        std::fs::create_dir_all(out_path)
            .with_context(|| format!("cannot create output dir {}", out_path.display()))?;
    }

    if matches!(args.format, OutputFormat::Json) {
        println!("{}", crate::text::envelope_header("image", json!({})));
    }
    let mut written = 0usize;
    let mut skipped_url = 0usize;
    for img in selected {
        if img.source_kind == "url" {
            // No inline bytes to write — report the URL, never fabricate a file.
            eprintln!(
                "csift: note: {} is a URL image ({}) — not extracted: {}",
                img.id(),
                img.media_type,
                img.url.as_deref().unwrap_or("<no url>")
            );
            skipped_url += 1;
            continue;
        }
        let b64 = img
            .data
            .as_deref()
            .with_context(|| format!("{}: image data missing (internal)", img.id()))?;
        let bytes = decode_base64(b64)
            .with_context(|| format!("{}: base64 decode failed (corrupt data)", img.id()))?;

        let src_fmt = format_of_media_type(&img.media_type);
        // FILE mode → convert to the path's format (raw if it already matches, keeping an animated
        // GIF animated); DIR mode → keep the source format (the "auto"/no-conversion path).
        let (out_bytes, out_ext, out_media, notes): (Vec<u8>, String, String, Vec<String>) =
            match file_target {
                None => (
                    bytes,
                    img.ext().to_string(),
                    img.media_type.clone(),
                    Vec::new(),
                ),
                Some(t) if src_fmt == Some(t) => (
                    bytes,
                    t.ext().to_string(),
                    t.media_type().to_string(),
                    Vec::new(),
                ),
                Some(t) => {
                    let (conv, notes) = convert_image(&bytes, t).with_context(|| {
                        format!(
                            "{}: convert {} → {}",
                            img.id(),
                            img.media_type,
                            t.media_type()
                        )
                    })?;
                    (conv, t.ext().to_string(), t.media_type().to_string(), notes)
                }
            };

        let path = if file_target.is_some() {
            out_path.to_path_buf()
        } else {
            out_path.join(img.out_filename_with_ext(&out_ext))
        };
        std::fs::write(&path, &out_bytes)
            .with_context(|| format!("cannot write {}", path.display()))?;
        match args.format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    json!({
                        "kind": "extract",
                        "handle": img.handle(), "seq": img.seq, "id": img.id(),
                        "path": path.to_string_lossy(),
                        "bytes": out_bytes.len(),
                        "media_type": out_media,
                        "source_media_type": img.media_type,
                        "converted": file_target.is_some() && src_fmt != file_target,
                        "notes": notes,
                        "session_id": img.session_id, "is_subagent": img.is_subagent,
                        "parent_session_id": img.parent_session_id,
                    })
                );
            }
            OutputFormat::Text => {
                let kind = if out_media == img.media_type {
                    out_media.clone()
                } else {
                    format!("{}→{out_media}", img.media_type)
                };
                let note = if notes.is_empty() {
                    String::new()
                } else {
                    format!("  — {}", notes.join("; "))
                };
                println!(
                    "wrote {}  ({} {}, {}){}",
                    path.display(),
                    img.handle(),
                    kind,
                    human_bytes(out_bytes.len()),
                    note
                );
            }
        }
        written += 1;
    }
    if matches!(args.format, OutputFormat::Json) {
        println!(
            "{}",
            crate::text::envelope_summary(json!({
                "extracted": written,
                "url_skipped": skipped_url,
                "skipped_lines": skipped_lines,
            }))
        );
    }
    if matches!(args.format, OutputFormat::Text) {
        let dest = if file_target.is_some() {
            String::new()
        } else {
            format!(" to {}", out_path.display())
        };
        let mut tail = format!("extracted {written} image(s){dest}");
        if skipped_url > 0 {
            tail.push_str(&format!(" · {skipped_url} url image(s) skipped"));
        }
        if skipped_lines > 0 {
            tail.push_str(&format!(
                " · ({})",
                crate::text::malformed_note(skipped_lines)
            ));
        }
        println!("{tail}");
    }
    Ok(())
}
