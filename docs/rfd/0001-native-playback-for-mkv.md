# RFD 0001: Native playback for Matroska titles

State: adopted (ADR 0001)
Authors: Ludo Galabru, astra  ·  Started: 2026-11-26

## Problem

72 percent of the movies on the owner's panel are `.mkv` (34,491 of 47,892,
counted 2026-11-26). AVFoundation has no Matroska demuxer, so the Apple
apps play these through VLCKit 3 (`apps/shared/Features/Player/VLCPlayer.swift`).
That works, but costs what the system player gives for free:

- no Picture in Picture and no AirPlay on iPhone;
- no Match Frame Rate or Match Dynamic Range on Apple TV, so HDR and 24p
  titles are shown converted;
- no Dolby Vision, no Atmos passthrough;
- our own controls instead of the system transport bar and info panel;
- about 34 MB of VLC per app.

## Sketch

Remux on the device, without transcoding: Matroska in, fragmented MP4 HLS
out, served to `AVPlayer` from a local HTTP server.

1. A `remux` module in Rust, reachable through `crates/ffi`, starts a
   loopback server (`127.0.0.1`, random port, random path token).
2. It reads the Matroska header and `Cues` (the keyframe index) with HTTP
   range requests, and writes an HLS media playlist with one segment per
   cue point, so seeking is a playlist lookup.
3. For each requested segment it range-reads the matching clusters from the
   panel and repackages the frames into fMP4 (`moof`/`mdat`), copying the
   compressed samples untouched. HEVC is tagged `hvc1` so AVFoundation
   accepts it.
4. Text subtitles (SRT, simple ASS) become WebVTT renditions in the
   playlist, so the system subtitle menu lists them.
5. The app hands `http://127.0.0.1:<port>/<token>/index.m3u8` to the
   existing `PlayerController`. Titles it can't remux go to VLC as today.

Codecs AVFoundation plays in fMP4: H.264, HEVC (including Dolby Vision
profiles 5 and 8), AAC, AC-3, E-AC-3 (Atmos), ALAC. Not playable: DTS,
TrueHD, VC-1, MPEG-2, and bitmap subtitles (PGS, VobSub); those titles keep
VLC.

Rough cost: a prototype (H.264/HEVC with AAC/AC-3, seeking, no subtitles)
is two to four days; something robust across a real library is one to two
weeks, most of it on timestamp and edge-case handling.

## Open questions

- **What share of the library is remuxable?** Probe the first megabyte of
  a few hundred `.mkv` titles with `ffprobe` (codecs live in the header)
  and count video, audio and subtitle codecs. Needs an idle connection.
- **Do the panel's servers honour `Range`?** Seeking depends on it. One
  `HEAD` and one ranged `GET` per server answer it.
- **Does the panel offer its own HLS for VOD?** Some Xtream panels serve
  `/movie/<user>/<pass>/<id>.m3u8`, remuxed server-side. If this one does,
  most of this RFD is unnecessary. One request answers it.
- **Pure Rust or FFmpeg?** A Matroska demuxer and an fMP4 writer are small
  enough to write (EBML is simple; fMP4 for this is a few boxes), which
  keeps the build free of FFmpeg's toolchain and licence. FFmpeg's
  `libavformat` is the safe fallback if edge cases pile up.

## Risks

- **One connection per plan.** AVPlayer fetches segments in parallel; the
  panel allows one stream. The proxy must hold the only upstream
  connection and serialise reads, or the panel refuses the second.
- **Start-up latency.** Reading `Cues` (often at the end of the file) costs
  a round trip before the first frame. Files without `Cues` need a scan or
  a VLC fallback.
- **Timestamps.** B-frames need composition offsets; variable frame rate
  and broken timestamps are common in pirated rips and are where remuxers
  spend their bugs.
- **Maintenance.** We would own a media pipeline. VLC already plays all of
  it; the gain is the system player's features, not playability.

## What we learned

### 2026-11-27: probes and prototype (opus)

- **The panel honours `Range`.** `/movie/USER/PASS/ID.mkv` answers 302 to
  a per-request CDN link, which answers 206 with `Content-Range`. The link
  can be reused for a while (three ranged reads at 0.2 to 0.35 s each), then
  answers 509. The proxy caches it and resolves a fresh one on failure.
- **The panel has no HLS for VOD.** `/movie/USER/PASS/ID.m3u8` answers 551.
- **97 percent of `.mkv` titles are remuxable.** `ffprobe` on the first 2 MB
  of 297 random titles (2 more failed): video H.264 261, HEVC Main 10 23,
  HEVC Main 9, MPEG-4 Part 2 4; audio tracks AAC 528, E-AC-3 168, AC-3 28,
  DTS 5, TrueHD 2, MP3 1. 288 titles have playable video and only playable
  audio. 5 carry Dolby Vision.
- **Audio and subtitle choice matters.** 93 titles (31 percent) have more
  than one audio track. Subtitle tracks: SRT 1,603, ASS 46, PGS 26,
  VobSub 3. So the prototype serves every audio track as a rendition and
  every text subtitle track as WebVTT, not one muxed audio track.
- **Select audio by bytes, not times.** ffmpeg's muxer stores audio up to
  ~100 ms after the keyframe cluster that starts the next segment. Taking
  each segment's frames from its byte range partitions every frame into
  exactly one segment; selecting by time dropped 8 of 625 E-AC-3 frames on
  the fixture.
- **Pure Rust holds.** EBML, Matroska, fMP4 and the HLS playlists took
  about 3,200 lines and no new dependency. On macOS, `AVPlayer` plays five
  real titles covering H.264, HEVC Main 10, Dolby Vision 8.1, AAC, AC-3,
  E-AC-3, 38 audio tracks and 32 subtitle tracks: ready in 0.75 to 1.5 s
  after a 1 to 2 s open, seeks in 0.3 to 1.2 s, 45 s of 4K without a stall.
  Subtitles render through `AVPlayerItemLegibleOutput` on time.
- **File colour tags lie, or are missing.** A Dolby Vision 8.1 title's
  `Colour` says BT.709, and HDR10 titles often carry no `Colour` at all.
  The range comes from the Dolby Vision record, then the HEVC SPS's VUI
  (`transfer_characteristics`), then the container tag; that order agreed
  with `ffprobe` on five HEVC titles.
- **Keyframes are not always at a cluster's start.** Some muxers cut
  clusters by time and write no `CueRelativePosition`: one title put two
  frames of the previous GOP before each keyframe, and cutting at clusters
  froze the picture for a quarter second every segment. Segments now split
  at the keyframe's block, from the index when it says, else found in the
  data with a 256 KiB look-ahead.
- **A sweep of 30 random titles:** 26 open in 2 to 7 s and serve every
  segment requested (first, middle, last; video, audio, subtitles) with
  clean decodes; 1 is Xvid (`V_MS/VFW/FOURCC`) and goes to VLC; 3 answer
  HTTP 405 to any request, dead at the panel.

The decision is [ADR 0001](../adr/0001-remux-matroska-to-hls-on-device.md).
