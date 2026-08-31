//! Narration-tagged thinking blocks: the signature decoder.
//!
//! Since at least CC 2.1.170 (2026-06-10) the API can return a SECOND `thinking` block
//! in an assistant message: a one-sentence, user-language summary of the reasoning
//! beside it (clients >= 2.1.241 render it dim under the hint word `summarized`).
//! Nothing distinguishes it on the wire except a tag encoded inside the base64
//! `signature`: protobuf field path 2 -> 1 -> 8, taking the LAST length-delimited field
//! at each level, yielding a UTF-8 string reading `narration` vs `thinking`.
//!
//! Classification is by SIGNATURE ONLY - a narration block can lack a reasoning sibling
//! entirely (~4% measured), so adjacency is never a shortcut. Signatures reach 200K+
//! base64 chars; the WHOLE string is decoded (a prefix decode silently reports every
//! long signature untagged). Every failure path - absent/empty signature, bad base64,
//! truncated varint, unknown wire type, non-UTF-8 - degrades to untagged (= plain
//! `agent.thinking`), never an error. The tag set is open: any value other than
//! `narration` classifies as plain thinking. The outer format-version varint is NEVER
//! consulted (values 2, 4 and absent all carry both tags in real data).

use super::*;

/// The tag value that marks a summary block.
pub(crate) const NARRATION_TAG: &str = "narration";

/// Decode the tag string hidden in a thinking block's `signature`; `None` when the
/// signature is absent or undecodable (untagged - treated exactly like tag `thinking`).
pub(crate) fn thinking_signature_tag(signature: Option<&str>) -> Option<String> {
    let sig = signature?;
    if sig.is_empty() {
        return None;
    }
    let bytes = crate::image::decode_base64(sig)?;
    let f2 = last_len_delimited(&bytes, 2)?;
    let f1 = last_len_delimited(f2, 1)?;
    let f8 = last_len_delimited(f1, 8)?;
    std::str::from_utf8(f8).ok().map(str::to_string)
}

/// The three base64 alignments of the tag bytes (`narration` at stream offset 0/1/2
/// mod 3). A signature containing NONE of them cannot decode to `narration`, so the
/// hot path skips the full decode - a µs substring check instead of decoding a 200KB
/// signature (classify runs on every thinking block of every candidate record; the
/// ungated form cost ~+20% wall on a 685MB transcript). Verified sound on 13,957 real
/// signatures (alignment MIXES within one file - all three needles are required).
/// Whitespace inside the base64 (tolerated by the decoder, never observed in real
/// signatures) would break needle adjacency, so it conservatively re-opens the gate.
const NARRATION_B64_ALIGNMENTS: [&str; 3] = ["bmFycmF0aW9u", "5hcnJhdGlv", "uYXJyYXRp"];

/// The leaf for a thinking block: `agent.thinking.narration` iff the signature tag reads
/// exactly `narration`; every other outcome (tag `thinking`, an unknown tag, no tag)
/// stays `agent.thinking`.
pub(crate) fn thinking_block_class(signature: Option<&str>) -> Class {
    let Some(sig) = signature else {
        return Class::AgentThinking;
    };
    let gate_open = NARRATION_B64_ALIGNMENTS.iter().any(|n| sig.contains(n))
        || sig.bytes().any(|b| b.is_ascii_whitespace());
    if !gate_open {
        return Class::AgentThinking;
    }
    if thinking_signature_tag(Some(sig)).as_deref() == Some(NARRATION_TAG) {
        Class::AgentThinkingNarration
    } else {
        Class::AgentThinking
    }
}

/// The payload of the LAST length-delimited (wire type 2) field numbered `want` at one
/// protobuf level. Mirrors the client decoder: varint (0) and fixed (1/5) fields are
/// skipped; an unknown wire type or any truncation aborts to `None` (untagged).
fn last_len_delimited(bytes: &[u8], want: u64) -> Option<&[u8]> {
    let mut pos = 0usize;
    let mut found: Option<&[u8]> = None;
    while pos < bytes.len() {
        let (key, n) = read_varint(&bytes[pos..])?;
        pos += n;
        let (field, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                let (_, n) = read_varint(&bytes[pos..])?;
                pos += n;
            }
            1 => pos = advance(pos, 8, bytes.len())?,
            5 => pos = advance(pos, 4, bytes.len())?,
            2 => {
                let (len, n) = read_varint(&bytes[pos..])?;
                pos += n;
                let end = advance(pos, usize::try_from(len).ok()?, bytes.len())?;
                if field == want {
                    found = Some(&bytes[pos..end]);
                }
                pos = end;
            }
            _ => return None,
        }
    }
    found
}

/// `pos + step`, bounds-checked against `len`.
fn advance(pos: usize, step: usize, len: usize) -> Option<usize> {
    let next = pos.checked_add(step)?;
    (next <= len).then_some(next)
}

/// A standard LEB128 varint: `(value, bytes consumed)`; `None` on truncation or an
/// over-long encoding. (High bits shifted past 64 are discarded, exactly like the
/// client's reader - the values read here are keys and lengths, never trusted numerics.)
fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, b) in bytes.iter().take(10).enumerate() {
        value |= u64::from(b & 0x7f) << (7 * i as u32);
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}
