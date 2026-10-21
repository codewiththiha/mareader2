//! A file's samples, decoded as they are asked for.

use std::io::Cursor;
use std::sync::Arc;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

use crate::Clip;

/// Undecodable packets skipped in a row before the file counts as ended.
const MAX_BAD_PACKETS: usize = 64;
/// Channels kept; more are dropped, never mixed into a crowd.
const MAX_CHANNELS: usize = 8;

/// A clip's bytes, shared with whoever read them.
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Decoded samples: when they start, their rate and their channels.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chunk {
    pub(crate) start: f64,
    pub(crate) rate: u32,
    pub(crate) channels: usize,
    /// Interleaved, `channels` to a frame.
    pub(crate) samples: Vec<f32>,
}

impl Chunk {
    pub(crate) fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
}

/// One file open for decoding.
pub(crate) struct Reader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    base: Option<TimeBase>,
    /// Where the next samples start, for files without timestamps.
    clock: f64,
    /// A seek's target: samples before it are dropped.
    want: Option<f64>,
    /// Samples handed back, given again first.
    pending: Option<Chunk>,
}

impl Reader {
    /// Opens `clip`; `None` if it isn't audio this reader decodes.
    pub(crate) fn open(clip: &Clip) -> Option<Self> {
        let source = Box::new(Cursor::new(Bytes(Arc::clone(&clip.data))));
        let stream = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        hint.with_extension(clip.extension);
        let (formats, meta) = (FormatOptions::default(), MetadataOptions::default());
        let probe = symphonia::default::get_probe();
        let format = probe.probe(&hint, stream, formats, meta).ok()?;
        let track = format.default_track(TrackType::Audio)?;
        let params = track.codec_params.as_ref()?.audio()?;
        let options = AudioDecoderOptions::default();
        let codecs = symphonia::default::get_codecs();
        let decoder = codecs.make_audio_decoder(params, &options).ok()?;
        let (track, base) = (track.id, track.time_base);
        Some(Self {
            format,
            decoder,
            track,
            base,
            clock: 0.0,
            want: None,
            pending: None,
        })
    }

    /// Goes to `at` seconds; the next chunk starts exactly there.
    pub(crate) fn seek(&mut self, at: f64) {
        let at = at.max(0.0);
        let time = Time::try_from_secs_f64(at).unwrap_or(Time::ZERO);
        let to = SeekTo::Time {
            time,
            track_id: Some(self.track),
        };
        // Files that can't seek are read from where they are.
        if let Ok(seeked) = self.format.seek(SeekMode::Accurate, to) {
            let landed = self.base.and_then(|b| b.calc_time(seeked.actual_ts));
            self.clock = landed.map_or(at, |t| t.as_secs_f64());
        }
        self.decoder.reset();
        self.want = Some(at);
        self.pending = None;
    }

    /// Gives `chunk` back, to come first from `next`.
    pub(crate) fn unread(&mut self, chunk: Chunk) {
        self.pending = Some(chunk);
    }

    /// Where the next samples start, in seconds.
    pub(crate) fn next_start(&self) -> f64 {
        self.pending.as_ref().map_or(self.clock, |c| c.start)
    }

    /// The next decoded samples; `None` at the file's end.
    pub(crate) fn next(&mut self) -> Option<Chunk> {
        if let Some(chunk) = self.pending.take() {
            return Some(chunk);
        }
        let mut bad = 0;
        while bad < MAX_BAD_PACKETS {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) | Err(_) => return None,
            };
            if packet.track_id != self.track {
                continue;
            }
            let stamped = self.base.and_then(|b| b.calc_time(packet.pts));
            let buffer = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(Error::DecodeError(_) | Error::IoError(_)) => {
                    bad += 1;
                    continue;
                }
                Err(_) => return None,
            };
            let rate = buffer.spec().rate();
            let channels = buffer.spec().channels().count();
            if rate == 0 || channels == 0 {
                bad += 1;
                continue;
            }
            let mut samples: Vec<f32> = Vec::new();
            buffer.copy_to_vec_interleaved(&mut samples);
            let start = stamped.map_or(self.clock, |t| t.as_secs_f64());
            let frames = samples.len() / channels;
            self.clock = start + frames as f64 / f64::from(rate);
            let mut chunk = Chunk {
                start,
                rate,
                channels,
                samples,
            };
            if channels > MAX_CHANNELS {
                chunk = narrowed(chunk);
            }
            let Some(want) = self.want else {
                return Some(chunk);
            };
            if let Some(kept) = trimmed(chunk, want) {
                self.want = None;
                return Some(kept);
            }
        }
        None
    }
}

/// `chunk` from `want` on; `None` if it ends before then.
fn trimmed(mut chunk: Chunk, want: f64) -> Option<Chunk> {
    let early = want - chunk.start;
    if early <= 0.0 {
        return Some(chunk);
    }
    let frames = (early * f64::from(chunk.rate)).round() as usize;
    if frames >= chunk.frames() {
        return None;
    }
    chunk.samples.drain(..frames * chunk.channels);
    chunk.start = want;
    Some(chunk)
}

/// `chunk` cut at `at` seconds: what comes before, and any after.
pub(crate) fn split(mut chunk: Chunk, at: f64) -> (Chunk, Option<Chunk>) {
    let keep = ((at - chunk.start) * f64::from(chunk.rate)).ceil().max(0.0) as usize;
    if keep >= chunk.frames() {
        return (chunk, None);
    }
    let after = chunk.samples.split_off(keep * chunk.channels);
    let rest = Chunk {
        start: chunk.start + keep as f64 / f64::from(chunk.rate),
        samples: after,
        ..chunk.clone()
    };
    (chunk, Some(rest))
}

/// Only the first `MAX_CHANNELS` of each frame.
fn narrowed(chunk: Chunk) -> Chunk {
    let samples = (chunk.samples.chunks_exact(chunk.channels))
        .flat_map(|frame| frame.iter().take(MAX_CHANNELS).copied())
        .collect();
    Chunk {
        samples,
        channels: MAX_CHANNELS,
        ..chunk
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 16-bit PCM WAV of `frames` frames whose samples count up.
    pub(crate) fn wav(rate: u32, channels: u16, frames: usize) -> Clip {
        let data_len = (frames * usize::from(channels) * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            for _ in 0..channels {
                let v = (i % 30000) as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Clip {
            data: Arc::new(out),
            extension: "wav",
        }
    }

    fn all(reader: &mut Reader) -> Vec<Chunk> {
        std::iter::from_fn(|| reader.next()).collect()
    }

    #[test]
    fn a_file_decodes_with_its_rate_channels_and_length() {
        let mut r = Reader::open(&wav(8000, 2, 8000)).unwrap();
        let chunks = all(&mut r);
        let frames: usize = chunks.iter().map(Chunk::frames).sum();
        assert_eq!(frames, 8000);
        assert!(chunks.iter().all(|c| c.rate == 8000 && c.channels == 2));
        assert_eq!(chunks[0].start, 0.0);
    }

    #[test]
    fn a_seek_lands_on_the_very_sample() {
        let mut r = Reader::open(&wav(8000, 1, 16000)).unwrap();
        r.seek(1.25);
        let first = r.next().unwrap();
        assert!((first.start - 1.25).abs() < 1e-9, "{}", first.start);
        // Sample 10000 holds 10000 / 32768 once read as a float.
        let expected = 10000.0 / 32768.0;
        assert!(
            (first.samples[0] - expected).abs() < 1e-4,
            "{}",
            first.samples[0]
        );
        let rest: usize = all(&mut r).iter().map(Chunk::frames).sum();
        assert_eq!(first.frames() + rest, 6000);
    }

    #[test]
    fn what_isnt_audio_doesnt_open() {
        let junk = Clip {
            data: Arc::new(b"zq not a sound file at all".to_vec()),
            extension: "mp3",
        };
        assert!(Reader::open(&junk).is_none());
    }
}
