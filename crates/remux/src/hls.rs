//! HLS playlists for a title: a multivariant playlist with one video
//! variant, an audio rendition per audio track and a subtitle rendition
//! per text track; then a media playlist for each.

use std::fmt::Write;

use crate::matroska::Track;
use crate::server::Route;
use crate::title::{Title, VideoRange};

/// Peak over average bit rate. Remuxes are VBR; Apple asks `BANDWIDTH`
/// for the peak, and twice the average covers typical encodes.
const PEAK_OVER_AVERAGE: u64 = 2;

pub fn multivariant(title: &Title) -> String {
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n");
    for (index, ((name, language), out)) in
        rendition_names(title.audio.iter().map(|out| &out.track))
            .into_iter()
            .zip(&title.audio)
            .enumerate()
    {
        writeln!(
            text,
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"{name}\",LANGUAGE=\"{language}\",DEFAULT={},AUTOSELECT=YES,CHANNELS=\"{}\",URI=\"{}\"",
            yes_no(index == 0),
            out.format.channels,
            Route::AudioPlaylist(index).path()
        )
        .expect("writing to a String cannot fail");
    }
    let subtitles = rendition_names(title.subtitles.iter().map(|out| &out.track));
    for (index, ((name, language), out)) in subtitles.into_iter().zip(&title.subtitles).enumerate()
    {
        writeln!(
            text,
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subtitles\",NAME=\"{name}\",LANGUAGE=\"{language}\",DEFAULT=NO,AUTOSELECT=YES,FORCED={},URI=\"{}\"",
            // Forced subtitles show by themselves when the audio matches.
            yes_no(out.track.is_forced),
            Route::SubtitlePlaylist(index).path()
        )
        .expect("writing to a String cannot fail");
    }
    let video = &title.video;
    let mut codecs = vec![video.codec_string.as_str()];
    for out in &title.audio {
        if !codecs.contains(&out.codec_string) {
            codecs.push(out.codec_string);
        }
    }
    let average = title.bytes_per_second * 8;
    write!(
        text,
        "#EXT-X-STREAM-INF:BANDWIDTH={},AVERAGE-BANDWIDTH={average},CODECS=\"{}\",RESOLUTION={}x{},VIDEO-RANGE={},AUDIO=\"audio\"",
        average * PEAK_OVER_AVERAGE,
        codecs.join(","),
        video.format.width,
        video.format.height,
        match video.range {
            VideoRange::Sdr => "SDR",
            VideoRange::Pq => "PQ",
            VideoRange::Hlg => "HLG",
        }
    )
    .expect("writing to a String cannot fail");
    if let Some(nanoseconds) = video.track.default_duration_ns {
        write!(text, ",FRAME-RATE={:.3}", 1e9 / nanoseconds as f64)
            .expect("writing to a String cannot fail");
    }
    if let Some(supplemental) = &video.supplemental_codec {
        write!(text, ",SUPPLEMENTAL-CODECS=\"{supplemental}\"")
            .expect("writing to a String cannot fail");
    }
    if !title.subtitles.is_empty() {
        text.push_str(",SUBTITLES=\"subtitles\"");
    }
    writeln!(text, "\n{}", Route::VideoPlaylist.path()).expect("writing to a String cannot fail");
    text
}

/// A media playlist: every segment of the title, `segment` naming each.
pub fn media(title: &Title, init: Option<Route>, segment: impl Fn(usize) -> Route) -> String {
    let longest_ns = title
        .segments
        .iter()
        .map(|span| span.duration_ns)
        .max()
        .expect("a title has segments");
    let target_seconds = longest_ns.div_ceil(1_000_000_000);
    let mut text = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target_seconds}\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n"
    );
    if let Some(init) = init {
        writeln!(text, "#EXT-X-MAP:URI=\"{}\"", init.path())
            .expect("writing to a String cannot fail");
    }
    for (index, span) in title.segments.iter().enumerate() {
        writeln!(
            text,
            "#EXTINF:{:.6},\n{}",
            span.duration_ns as f64 / 1e9,
            segment(index).path()
        )
        .expect("writing to a String cannot fail");
    }
    text.push_str("#EXT-X-ENDLIST\n");
    text
}

fn yes_no(value: bool) -> &'static str {
    if value { "YES" } else { "NO" }
}

/// `(NAME, LANGUAGE)` per track. Names must be unique within a group, so
/// repeats get a number.
fn rendition_names<'a>(tracks: impl Iterator<Item = &'a Track>) -> Vec<(String, String)> {
    let mut seen: Vec<String> = Vec::new();
    tracks
        .map(|track| {
            let language = bcp47(&track.language);
            let base = track
                .name
                .clone()
                .unwrap_or_else(|| language_name(&language).to_owned())
                .replace('"', "'");
            let count = seen.iter().filter(|name| **name == base).count();
            seen.push(base.clone());
            let name = if count == 0 {
                base
            } else {
                format!("{base} {}", count + 1)
            };
            (name, language)
        })
        .collect()
}

/// ISO 639-2 codes, as most Matroska files tag languages, to the BCP 47
/// tags HLS wants. Codes not listed pass through: still valid BCP 47.
const LANGUAGES: [(&[&str], &str, &str); 20] = [
    (&["eng"], "en", "English"),
    (&["fre", "fra"], "fr", "Français"),
    (&["spa"], "es", "Español"),
    (&["ger", "deu"], "de", "Deutsch"),
    (&["ita"], "it", "Italiano"),
    (&["por"], "pt", "Português"),
    (&["dut", "nld"], "nl", "Nederlands"),
    (&["ara"], "ar", "العربية"),
    (&["tur"], "tr", "Türkçe"),
    (&["rus"], "ru", "Русский"),
    (&["pol"], "pl", "Polski"),
    (&["jpn"], "ja", "日本語"),
    (&["kor"], "ko", "한국어"),
    (&["chi", "zho"], "zh", "中文"),
    (&["hin"], "hi", "हिन्दी"),
    (&["swe"], "sv", "Svenska"),
    (&["dan"], "da", "Dansk"),
    (&["nor", "nob"], "no", "Norsk"),
    (&["fin"], "fi", "Suomi"),
    (&["gre", "ell"], "el", "Ελληνικά"),
];

fn bcp47(language: &str) -> String {
    let code = language.to_ascii_lowercase();
    if code.is_empty() || code == "und" {
        return String::from("und");
    }
    LANGUAGES
        .iter()
        .find(|(codes, _, _)| codes.contains(&code.as_str()))
        .map_or(language.to_owned(), |(_, tag, _)| (*tag).to_owned())
}

fn language_name(tag: &str) -> &str {
    let primary = tag.split('-').next().unwrap_or(tag);
    match LANGUAGES
        .iter()
        .find(|(_, candidate, _)| *candidate == primary)
    {
        Some((_, _, name)) => name,
        None if tag == "und" => "Unknown",
        None => tag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_matroska_languages_to_bcp47() {
        assert_eq!(bcp47("fre"), "fr");
        assert_eq!(bcp47("fr-CA"), "fr-CA");
        assert_eq!(bcp47("und"), "und");
        assert_eq!(bcp47("tlh"), "tlh");
        assert_eq!(language_name("fr-CA"), "Français");
        assert_eq!(language_name("und"), "Unknown");
    }
}
