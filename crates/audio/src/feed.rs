//! Segments turned into device samples, each run tagged with its time.

use std::collections::VecDeque;

use crate::decode::{self, Reader};
use crate::resample::Resampler;
use crate::stretch::Stretch;
use crate::{Clip, Fault, Fetch, Segment};

/// Gaps between segments smaller than this play straight on, in seconds.
const SEAMLESS: f64 = 0.002;

/// A run of samples in the ring: what it plays and from when.
#[derive(Debug, Clone, PartialEq)]
struct Mark {
    frames: usize,
    tag: u64,
    time: f64,
    /// File seconds per frame heard, so speed included.
    step: f64,
}

/// Samples waiting for the device, with what they are.
#[derive(Debug, Default)]
pub(crate) struct Ring {
    channels: usize,
    samples: VecDeque<f32>,
    marks: VecDeque<Mark>,
    /// What the last sample heard was.
    now: Option<(u64, f64)>,
    /// No more samples are coming.
    ended: bool,
}

impl Ring {
    pub(crate) fn clear(&mut self) {
        self.samples.clear();
        self.marks.clear();
        self.now = None;
        self.ended = false;
    }

    /// Frames waiting.
    pub(crate) fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub(crate) fn now(&self) -> Option<(u64, f64)> {
        self.now
    }

    /// No more samples are coming.
    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    pub(crate) fn done(&self) -> bool {
        self.ended && self.samples.is_empty()
    }

    pub(crate) fn accept(&mut self, batch: Batch) {
        for (samples, mark) in batch.runs {
            self.push(batch.channels, samples, mark);
        }
        self.ended |= batch.ended;
    }

    fn push(&mut self, channels: usize, samples: Vec<f32>, mark: Mark) {
        if self.channels != channels {
            self.samples.clear();
            self.marks.clear();
            self.channels = channels;
        }
        if mark.frames > 0 {
            self.samples.extend(samples);
            self.marks.push_back(mark);
        }
    }

    /// Fills `out` from the front, silence past the end.
    pub(crate) fn take(&mut self, out: &mut [f32]) {
        let ch = self.channels.max(1);
        let mut wanted = out.len() / ch;
        let mut slots = out.iter_mut();
        while wanted > 0 {
            let Some(mark) = self.marks.front_mut() else {
                break;
            };
            let n = mark.frames.min(wanted);
            for (slot, s) in slots.by_ref().zip(self.samples.drain(..n * ch)) {
                *slot = s;
            }
            mark.frames -= n;
            mark.time += mark.step * n as f64;
            self.now = Some((mark.tag, mark.time));
            if mark.frames == 0 {
                self.marks.pop_front();
            }
            wanted -= n;
        }
        for slot in slots {
            *slot = 0.0;
        }
    }
}

/// Runs decoded away from the device's lock, to add in one go.
#[derive(Debug, Default)]
pub(crate) struct Batch {
    channels: usize,
    runs: Vec<(Vec<f32>, Mark)>,
    ended: bool,
}

/// The file being decoded now, and how far into its segment.
struct Open {
    path: String,
    reader: Reader,
    segment: Segment,
    resampler: Resampler,
    stretch: Option<Stretch>,
    /// Device-rate frames given since the segment began.
    given: usize,
    /// File seconds at the segment's first frame.
    start: f64,
}

/// Decodes queued segments into device-ready samples.
pub(crate) struct Feed {
    fetch: Fetch,
    rate: u32,
    channels: usize,
    speed: f32,
    queue: VecDeque<Segment>,
    open: Option<Open>,
    /// The last file read, kept to go on without opening it again.
    spare: Option<(String, Reader)>,
    pub(crate) fault: Option<Fault>,
}

impl Feed {
    /// For a device playing `channels` at `rate`.
    pub(crate) fn new(fetch: Fetch, rate: u32, channels: usize) -> Self {
        Self {
            fetch,
            rate: rate.max(1),
            channels: channels.max(1),
            speed: 1.0,
            queue: VecDeque::new(),
            open: None,
            spare: None,
            fault: None,
        }
    }

    /// Plays `segments` from the first, dropping the rest.
    pub(crate) fn play(&mut self, segments: Vec<Segment>) {
        self.queue = segments.into();
        self.close();
        self.fault = None;
    }

    /// Goes on at `speed` from `at`, a segment and its file time.
    pub(crate) fn set_speed(&mut self, speed: f32, at: Option<(u64, f64)>) {
        self.speed = speed.clamp(0.25, 4.0);
        let Some((tag, time)) = at else {
            return;
        };
        // The segment heard comes back from where it was heard.
        let mut rest: Vec<Segment> = Vec::new();
        if let Some(open) = self.open.as_ref().filter(|o| o.segment.tag == tag) {
            rest.push(Segment {
                from: time,
                ..open.segment.clone()
            });
        }
        rest.extend(self.queue.drain(..));
        self.play(rest);
    }

    pub(crate) fn stop(&mut self) {
        self.queue.clear();
        self.close();
    }

    fn close(&mut self) {
        if let Some(open) = self.open.take() {
            self.spare = Some((open.path, open.reader));
        }
    }

    /// Nothing is left to decode.
    pub(crate) fn ended(&self) -> bool {
        self.open.is_none() && self.queue.is_empty()
    }

    /// The next run of samples and what it plays; `None` once ended.
    fn produce(&mut self) -> Option<(Vec<f32>, Mark)> {
        loop {
            if self.open.is_none() {
                let segment = self.queue.pop_front()?;
                self.open = self.begin(segment);
                continue;
            }
            let open = self.open.as_mut()?;
            let next = open.reader.next();
            // What lies past the segment's end waits for the next one.
            let chunk = match (next, open.segment.until) {
                (Some(c), Some(until)) if c.start >= until - 1e-9 => {
                    open.reader.unread(c);
                    None
                }
                (Some(c), Some(until)) => {
                    let (head, rest) = decode::split(c, until);
                    if let Some(rest) = rest {
                        open.reader.unread(rest);
                    }
                    Some(head)
                }
                (next, _) => next,
            };
            let Some(chunk) = chunk else {
                let tail = open
                    .stretch
                    .as_mut()
                    .map(Stretch::finish)
                    .unwrap_or_default();
                let mark = self.mark_for(tail.len());
                self.close();
                match mark {
                    Some(mark) => return Some((tail, mark)),
                    None => continue,
                }
            };
            let at = open
                .stretch
                .as_ref()
                .map_or(open.given as f64, Stretch::position);
            let resampled =
                open.resampler
                    .push(&chunk.samples, chunk.channels, chunk.rate, self.rate);
            open.given += resampled.len() / self.channels;
            let out = match open.stretch.as_mut() {
                Some(s) => s.push(&resampled),
                None => resampled,
            };
            let frames = out.len() / self.channels;
            if frames == 0 {
                continue;
            }
            let rate = f64::from(self.rate);
            let mark = Mark {
                frames,
                tag: open.segment.tag,
                time: open.start + at / rate,
                step: f64::from(self.speed) / rate,
            };
            return Some((out, mark));
        }
    }

    /// A mark for `len` samples closing the open segment.
    fn mark_for(&self, len: usize) -> Option<Mark> {
        let open = self.open.as_ref()?;
        let frames = len / self.channels;
        let at = open
            .stretch
            .as_ref()
            .map_or(open.given as f64, Stretch::position);
        let rate = f64::from(self.rate);
        (frames > 0).then(|| Mark {
            frames,
            tag: open.segment.tag,
            time: open.start + at / rate,
            step: f64::from(self.speed) / rate,
        })
    }

    /// Opens `segment`'s file, or goes on in the last one if it can.
    fn begin(&mut self, segment: Segment) -> Option<Open> {
        let spare = self.spare.take();
        let (reader, start) = match spare {
            Some((path, reader))
                if path == segment.path
                    && (reader.next_start() - segment.from).abs() < SEAMLESS =>
            {
                let start = reader.next_start();
                (reader, start)
            }
            _ => {
                let clip: Option<Clip> = (self.fetch)(&segment.path);
                let reader = clip.as_ref().and_then(Reader::open);
                let Some(mut reader) = reader else {
                    self.fault = Some(Fault::Unreadable(segment.path));
                    return None;
                };
                if segment.from > 0.0 {
                    reader.seek(segment.from);
                }
                (reader, segment.from.max(0.0))
            }
        };
        let stretch =
            (self.speed != 1.0).then(|| Stretch::new(self.rate, self.channels, self.speed));
        Some(Open {
            path: segment.path.clone(),
            reader,
            segment,
            resampler: Resampler::new(self.channels),
            stretch,
            given: 0,
            start,
        })
    }

    /// Adds runs to `ring` until it holds `ahead` frames or all is in.
    #[cfg(test)]
    pub(crate) fn fill(&mut self, ring: &mut Ring, ahead: usize) {
        let batch = self.batch(ahead.saturating_sub(ring.frames()));
        ring.accept(batch);
    }

    /// Runs worth at least `frames` frames, or all that is left.
    pub(crate) fn batch(&mut self, frames: usize) -> Batch {
        let mut batch = Batch {
            channels: self.channels,
            runs: Vec::new(),
            ended: false,
        };
        let mut made = 0;
        while made < frames {
            match self.produce() {
                Some((samples, mark)) => {
                    made += mark.frames;
                    batch.runs.push((samples, mark));
                }
                None => {
                    batch.ended = true;
                    break;
                }
            }
        }
        batch
    }

    /// Everything left, as one stream.
    #[cfg(test)]
    pub(crate) fn drain(&mut self) -> (Vec<f32>, Vec<(u64, f64)>) {
        let mut samples = Vec::new();
        let mut marks = Vec::new();
        while let Some((s, m)) = self.produce() {
            samples.extend(s);
            marks.push((m.tag, m.time));
        }
        (samples, marks)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::decode::tests::wav;

    fn feed(rate: u32) -> Feed {
        let fetch: Fetch = Arc::new(|path: &str| match path {
            "a.wav" => Some(wav(8000, 1, 16000)),
            "b.wav" => Some(wav(16000, 2, 16000)),
            _ => None,
        });
        Feed::new(fetch, rate, 2)
    }

    fn segment(path: &str, from: f64, until: Option<f64>, tag: u64) -> Segment {
        Segment {
            path: path.into(),
            from,
            until,
            tag,
        }
    }

    #[test]
    fn a_segment_plays_from_its_start_to_its_end() {
        let mut f = feed(8000);
        f.play(vec![segment("a.wav", 0.5, Some(1.5), 7)]);
        let (samples, marks) = f.drain();
        // A second of 8 kHz, spread over two channels.
        assert_eq!(samples.len(), 16000);
        assert_eq!(marks[0], (7, 0.5));
        assert!(marks.iter().all(|m| m.0 == 7 && (0.5..1.5).contains(&m.1)));
    }

    #[test]
    fn segments_follow_on_across_files_and_rates() {
        let mut f = feed(8000);
        let segments = vec![
            segment("a.wav", 0.0, Some(0.25), 1),
            segment("b.wav", 0.5, None, 2),
        ];
        f.play(segments);
        let (samples, marks) = f.drain();
        // b is 16 kHz: its last half second comes down to 4000 frames.
        let frames = samples.len() / 2;
        assert!((5990..=6010).contains(&frames), "{frames}");
        let first_b = marks.iter().find(|m| m.0 == 2).unwrap();
        assert!((first_b.1 - 0.5).abs() < 1e-6, "{first_b:?}");
    }

    #[test]
    fn touching_segments_read_on_without_a_seek() {
        let mut f = feed(8000);
        let segments = vec![
            segment("a.wav", 0.0, Some(0.5), 1),
            segment("a.wav", 0.5, Some(1.0), 2),
        ];
        f.play(segments);
        let (samples, marks) = f.drain();
        assert_eq!(samples.len() / 2, 8000);
        let tags: Vec<u64> = marks.iter().map(|m| m.0).collect();
        assert!(tags.contains(&1) && tags.contains(&2), "{tags:?}");
    }

    #[test]
    fn a_missing_file_is_a_fault_and_the_rest_play() {
        let mut f = feed(8000);
        f.play(vec![
            segment("gone.mp3", 0.0, None, 1),
            segment("a.wav", 1.5, None, 2),
        ]);
        let (samples, _) = f.drain();
        assert_eq!(samples.len() / 2, 4000);
        assert_eq!(f.fault, Some(Fault::Unreadable("gone.mp3".into())));
    }

    #[test]
    fn faster_speech_takes_less_time_and_keeps_its_place() {
        let mut f = feed(8000);
        f.set_speed(2.0, None);
        f.play(vec![segment("a.wav", 0.0, None, 3)]);
        let (samples, marks) = f.drain();
        let frames = samples.len() / 2;
        assert!((7800..=8200).contains(&frames), "{frames}");
        let last = marks.last().unwrap().1;
        assert!(last > 1.8 && last <= 2.0, "{last}");
    }

    #[test]
    fn the_ring_reports_what_is_heard() {
        let mut f = feed(8000);
        f.play(vec![segment("a.wav", 1.0, None, 9)]);
        let mut ring = Ring::default();
        f.fill(&mut ring, 4000);
        let mut out = vec![0.0; 2 * 800];
        ring.take(&mut out);
        let (tag, time) = ring.now().unwrap();
        assert_eq!(tag, 9);
        assert!((time - 1.1).abs() < 1e-6, "{time}");
        assert!(!ring.done());
    }

    #[test]
    fn muted_sound_is_silent_but_keeps_its_time() {
        let mut f = feed(8000);
        f.play(vec![segment("a.wav", 1.0, None, 9)]);
        let mut shared = crate::Shared {
            muted: true,
            ..Default::default()
        };
        f.fill(&mut shared.ring, 4000);
        let mut out = vec![1.0; 2 * 800];
        shared.take(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        let (_, time) = shared.ring.now().unwrap();
        assert!((time - 1.1).abs() < 1e-6, "{time}");
    }
}
