//! Text subtitles to WebVTT cue text.

use crate::codec::SubtitleCodec;

/// Fields before the text in an ASS event as Matroska stores it:
/// ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV, Effect.
const ASS_FIELDS_BEFORE_TEXT: usize = 8;
/// Tags WebVTT renders; any other tag is dropped, its text kept.
const WEBVTT_TAGS: [&str; 3] = ["i", "b", "u"];

/// The cue text of one subtitle frame, empty when nothing is left to show.
pub fn to_webvtt_text(raw: &[u8], codec: SubtitleCodec) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = match codec {
        // SubRip files converted from ASS often keep `{\an8}` tags.
        SubtitleCodec::Srt => without_overrides(&text).replace("\r\n", "\n"),
        SubtitleCodec::Ass => ass_text(&text),
    };
    let text = keep_webvtt_tags(&text);
    // A blank line ends a cue, and "-->" starts a timing line.
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .replace("-->", "->")
}

fn ass_text(event: &str) -> String {
    let text = event
        .splitn(ASS_FIELDS_BEFORE_TEXT + 1, ',')
        .nth(ASS_FIELDS_BEFORE_TEXT)
        .unwrap_or("");
    without_overrides(text)
        .replace("\\N", "\n")
        .replace("\\n", "\n")
        .replace("\\h", " ")
}

/// Drops ASS style overrides, `{\...}`; other braces are text.
fn without_overrides(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("{\\") {
        out.push_str(&rest[..at]);
        match rest[at..].find('}') {
            Some(close) => rest = &rest[at + close + 1..],
            None => {
                rest = &rest[at..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes `&` and `<` except in `<i>`, `<b>`, `<u>` and their closings;
/// drops other tags (`<font color=...>`).
fn keep_webvtt_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['<', '&']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('&') {
            out.push_str("&amp;");
            rest = &rest[1..];
            continue;
        }
        let Some(close) = rest.find('>') else {
            out.push_str("&lt;");
            rest = &rest[1..];
            continue;
        };
        let tag = rest[1..close].trim_start_matches('/').to_ascii_lowercase();
        if WEBVTT_TAGS.contains(&tag.as_str()) {
            out.push_str(&rest[..=close].to_ascii_lowercase());
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// `HH:MM:SS.mmm`.
pub fn timestamp(ns: u64) -> String {
    let ms = ns / 1_000_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_keeps_italics_and_drops_fonts() {
        let raw = "<i>Bonjour</i> & <font color=\"#ff0\">salut</font>\r\n\r\n-- fin --> ici";
        assert_eq!(
            to_webvtt_text(raw.as_bytes(), SubtitleCodec::Srt),
            "<i>Bonjour</i> &amp; salut\n-- fin -> ici"
        );
    }

    #[test]
    fn srt_drops_ass_overrides_but_keeps_other_braces() {
        assert_eq!(
            to_webvtt_text(b"{\\an8}TOUS {DECEDES}", SubtitleCodec::Srt),
            "TOUS {DECEDES}"
        );
        assert_eq!(
            to_webvtt_text(b"open {\\i1", SubtitleCodec::Srt),
            "open {\\i1"
        );
    }

    #[test]
    fn srt_escapes_a_lone_angle_bracket() {
        assert_eq!(to_webvtt_text(b"1 < 2", SubtitleCodec::Srt), "1 &lt; 2");
    }

    #[test]
    fn ass_keeps_the_text_field_without_overrides() {
        let raw = "12,0,Default,,0,0,0,,{\\an8}Hello,\\Nworld{\\i1}!";
        assert_eq!(
            to_webvtt_text(raw.as_bytes(), SubtitleCodec::Ass),
            "Hello,\nworld!"
        );
        assert_eq!(to_webvtt_text(b"too,few,fields", SubtitleCodec::Ass), "");
    }

    #[test]
    fn formats_hours_minutes_seconds_milliseconds() {
        assert_eq!(timestamp(3_723_004_000_000), "01:02:03.004");
    }
}
