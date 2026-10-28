//! Local audio file decoding (symphonia) to the player's fixed format:
//! interleaved f32 at `PLAYER_RATE`, `PLAYER_CHANNELS`.

use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::audio::resample::CaptureConverter;

pub const PLAYER_RATE: u32 = 48_000;
pub const PLAYER_CHANNELS: usize = 2;

/// Extensions offered in the file picker; all decode with the bundled codecs.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "mp4"];

/// Corrupt packets are skipped; this many in a row means the file is unusable.
const MAX_CONSECUTIVE_DECODE_ERRORS: u32 = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct TrackMeta {
    pub title: String,
    pub duration_secs: Option<f64>,
}

struct Opened {
    format: Box<dyn FormatReader>,
    track_id: u32,
    decoder: Box<dyn Decoder>,
    time_base: Option<TimeBase>,
    duration_secs: Option<f64>,
    title: String,
}

fn secs(t: Time) -> f64 {
    t.seconds as f64 + t.frac
}

fn tags_title(rev: &MetadataRevision) -> Option<String> {
    let find = |key: StandardTagKey| {
        rev.tags()
            .iter()
            .find(|t| t.std_key == Some(key))
            .map(|t| t.value.to_string().trim().to_string())
            .filter(|s| !s.is_empty())
    };
    match (find(StandardTagKey::Artist), find(StandardTagKey::TrackTitle)) {
        (Some(a), Some(t)) => Some(format!("{a} – {t}")),
        (None, Some(t)) => Some(t),
        _ => None,
    }
}

fn open(path: &Path) -> Result<Opened, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Unsupported or damaged audio file: {e}"))?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "No audio track in file".to_string())?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("Unsupported audio codec: {e}"))?;

    let duration_secs = match (params.time_base, params.n_frames) {
        (Some(tb), Some(n)) => Some(secs(tb.calc_time(n))),
        (None, Some(n)) => params.sample_rate.map(|r| n as f64 / r as f64),
        _ => None,
    };

    // ID3v2 and friends come before the container; Vorbis/MP4 tags inside it.
    let mut title = probed.metadata.get().as_ref().and_then(|m| m.current().and_then(tags_title));
    if title.is_none() {
        title = probed.format.metadata().current().and_then(tags_title);
    }
    let title = title.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    });

    Ok(Opened {
        format: probed.format,
        track_id,
        decoder,
        time_base: params.time_base,
        duration_secs,
        title,
    })
}

/// Read title and duration without decoding audio.
pub fn probe_file(path: &Path) -> Result<TrackMeta, String> {
    let o = open(path)?;
    Ok(TrackMeta {
        title: o.title,
        duration_secs: o.duration_secs,
    })
}

pub struct FileDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    duration_secs: Option<f64>,
    sample_buf: Option<SampleBuffer<f32>>,
    converter: Option<(CaptureConverter, u32, usize)>,
    pending: VecDeque<f32>,
    /// Output frames since `base_secs` (the last seek target).
    frames_out: u64,
    base_secs: f64,
    ended: bool,
}

impl FileDecoder {
    pub fn open(path: &Path) -> Result<Self, String> {
        let o = open(path)?;
        Ok(Self {
            format: o.format,
            decoder: o.decoder,
            track_id: o.track_id,
            time_base: o.time_base,
            duration_secs: o.duration_secs,
            sample_buf: None,
            converter: None,
            pending: VecDeque::new(),
            frames_out: 0,
            base_secs: 0.0,
            ended: false,
        })
    }

    pub fn duration_secs(&self) -> Option<f64> {
        self.duration_secs
    }

    pub fn position_secs(&self) -> f64 {
        self.base_secs + self.frames_out as f64 / PLAYER_RATE as f64
    }

    /// Fill `out` (interleaved, `PLAYER_CHANNELS`) and return the frames written.
    /// Fewer than requested means the end of the file.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let want = out.len() - out.len() % PLAYER_CHANNELS;
        while self.pending.len() < want && !self.ended {
            if !self.decode_packet() {
                // Push the resampler's buffered tail out with a little silence.
                if let Some((conv, _, ch)) = self.converter.as_mut() {
                    let pad = vec![0.0; CaptureConverter::CHUNK_FRAMES * 2 * *ch];
                    self.pending.extend(conv.process_f32(&pad));
                }
                self.ended = true;
            }
        }
        let n = want.min(self.pending.len() - self.pending.len() % PLAYER_CHANNELS);
        for (dst, src) in out[..n].iter_mut().zip(self.pending.drain(..n)) {
            *dst = src;
        }
        let frames = n / PLAYER_CHANNELS;
        self.frames_out += frames as u64;
        frames
    }

    /// Decode one packet into `pending`. False at end of stream (or a fatal error).
    fn decode_packet(&mut self) -> bool {
        let mut errors = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return false,
                Err(e) => {
                    tracing::warn!("playlist: read error, ending track: {e}");
                    return false;
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(audio) => {
                    let spec = *audio.spec();
                    let channels = spec.channels.count().max(1);
                    let buf = match &mut self.sample_buf {
                        Some(b) if b.capacity() >= audio.capacity() => b,
                        slot => slot.insert(SampleBuffer::<f32>::new(audio.capacity() as u64, spec)),
                    };
                    buf.copy_interleaved_ref(audio);

                    let reuse = matches!(&self.converter, Some((_, r, c)) if *r == spec.rate && *c == channels);
                    if !reuse {
                        match CaptureConverter::new(spec.rate, channels, PLAYER_RATE, PLAYER_CHANNELS) {
                            Ok(c) => self.converter = Some((c, spec.rate, channels)),
                            Err(e) => {
                                tracing::warn!("playlist: {e}");
                                return false;
                            }
                        }
                    }
                    if let Some((conv, _, _)) = self.converter.as_mut() {
                        self.pending.extend(conv.process_f32(buf.samples()));
                    }
                    return true;
                }
                Err(SymError::DecodeError(e)) => {
                    errors += 1;
                    if errors > MAX_CONSECUTIVE_DECODE_ERRORS {
                        tracing::warn!("playlist: too many decode errors, ending track: {e}");
                        return false;
                    }
                }
                Err(e) => {
                    tracing::warn!("playlist: decoder error, ending track: {e}");
                    return false;
                }
            }
        }
    }

    pub fn seek(&mut self, to_secs: f64) -> Result<(), String> {
        let mut target = to_secs.max(0.0);
        if let Some(d) = self.duration_secs {
            target = target.min((d - 0.25).max(0.0));
        }
        let seeked = self
            .format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(target),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| format!("Cannot seek in this file: {e}"))?;
        self.decoder.reset();
        self.pending.clear();
        self.converter = None;
        self.ended = false;
        self.frames_out = 0;
        self.base_secs = match self.time_base {
            Some(tb) => secs(tb.calc_time(seeked.actual_ts)),
            None => target,
        };
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal 16-bit PCM WAV with a sine tone.
    pub(crate) fn write_wav(path: &Path, rate: u32, channels: u16, secs: f32) {
        let frames = (rate as f32 * secs) as u32;
        let data_len = frames * channels as u32 * 2;
        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * channels as u32 * 2).to_le_bytes()).unwrap();
        f.write_all(&(channels * 2).to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for i in 0..frames {
            let s = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 12_000.0) as i16;
            for _ in 0..channels {
                f.write_all(&s.to_le_bytes()).unwrap();
            }
        }
    }

    #[test]
    fn probe_reads_duration_and_falls_back_to_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Kajian Subuh.wav");
        write_wav(&p, 22_050, 1, 1.5);
        let meta = probe_file(&p).unwrap();
        assert_eq!(meta.title, "Kajian Subuh");
        let d = meta.duration_secs.unwrap();
        assert!((d - 1.5).abs() < 0.01, "duration {d}");
    }

    #[test]
    fn decoder_outputs_the_whole_file_at_the_player_format() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, 44_100, 1, 1.0);
        let mut dec = FileDecoder::open(&p).unwrap();
        let mut total = 0usize;
        let mut buf = vec![0.0f32; 960 * PLAYER_CHANNELS];
        loop {
            let n = dec.read(&mut buf);
            total += n;
            if n < 960 {
                break;
            }
        }
        // 1 s at 48 kHz, plus at most the flushed resampler padding.
        assert!((47_000..=48_000 + 2 * CaptureConverter::CHUNK_FRAMES * 2).contains(&total), "{total}");
        assert!((dec.position_secs() - total as f64 / 48_000.0).abs() < 1e-9);
    }

    #[test]
    fn seek_moves_the_position() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.wav");
        write_wav(&p, 48_000, 2, 3.0);
        let mut dec = FileDecoder::open(&p).unwrap();
        dec.seek(2.0).unwrap();
        assert!((dec.position_secs() - 2.0).abs() < 0.05, "{}", dec.position_secs());
        let mut buf = vec![0.0f32; 48_000 * PLAYER_CHANNELS];
        let n = dec.read(&mut buf);
        assert!(n > 40_000 && n < 49_000, "read {n} frames after seeking to 2 s of 3 s");
    }

    #[test]
    fn garbage_is_rejected_with_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.mp3");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(probe_file(&p).is_err());
    }
}
