# ADR 0001: Play Matroska in the system player by remuxing it to HLS on the device

Status: accepted (2026-11-27)

## Context

72 percent of the panel's movies are `.mkv`, and AVFoundation has no
Matroska demuxer. Until now the apps played them through VLCKit, without
Picture in Picture, AirPlay, Match Frame Rate, Match Dynamic Range or the
system transport bar. [RFD 0001](../rfd/0001-native-playback-for-mkv.md)
proposed remuxing on the device; its "What we learned" section holds the
measurements this decision rests on.

The panel serves no HLS of its own for VOD (HTTP 551 on `.m3u8`), but it
honours `Range`. Of 297 random `.mkv` titles, 288 (97 percent) hold only
codecs AVFoundation plays from fragmented MP4: H.264 or HEVC, with AAC,
AC-3 or E-AC-3.

## Decision

The `televisor-remux` crate turns a Matroska title into HLS without
transcoding, served from a loopback HTTP server under a random path token.
It reads the header and `Cues` with range requests, cuts one segment per
run of keyframes of about 6 s, and repackages each segment's frames into
fragmented MP4. Every playable audio track is an audio rendition and every
text subtitle track (SRT, ASS) a WebVTT rendition, so the system menus list
them. Upstream reads are serialised on one connection, as the plan allows.

We write the demuxer and the MP4 writer ourselves, in Rust, with no new
dependency. FFmpeg's `libavformat` stays the fallback if edge cases pile up.

The apps call `remux_open` for `.mkv` titles and play the returned URL in
`AVPlayer`. When the remuxer refuses a title, or AVPlayer fails on it, the
title plays in VLC as before.

## Consequences

- Most of the library gains the system player's features. Dolby Vision
  profiles 5 and 8 are signalled, so Apple TV can switch modes.
- We own a media pipeline (about 3,200 lines of Rust): timestamps, lacing,
  codec headers. Its tests cover a committed ffmpeg fixture end to end.
- VLC stays in the apps for DTS, TrueHD, MPEG-4 Part 2, bitmap subtitles,
  files without `Cues`, and other containers.
- AirPlay can't hand a loopback URL to a receiver, so remuxed titles send
  rendered video instead of the stream.
- A segment is read whole before it is served, and up to three are kept in
  memory, 30 MB in practice and 384 MB at the read cap.

## Possible future improvements

- Serve AirPlay receivers on the LAN address instead of loopback.
- Signal Dolby Atmos (E-AC-3 JOC) in `dec3`, and 7.1 dependent substreams.
- Prefetch the next segment while AVPlayer decodes the current one.
- DTS and TrueHD can't be remuxed for AVFoundation; decoding them to PCM
  in the remuxer would be the next step if those titles matter.

## References

- `crates/remux/src/lib.rs`: the module map.
- Apple, HLS authoring specification for Apple devices (segment length,
  `VIDEO-RANGE`, `SUPPLEMENTAL-CODECS`).
