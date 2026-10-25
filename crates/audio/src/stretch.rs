//! Faster or slower speech at the same pitch, by overlapping pieces.

/// The piece length, in seconds.
const WINDOW: f64 = 0.030;
/// How far a piece may be nudged either way, in seconds.
const TOLERANCE: f64 = 0.008;

/// Each step takes a piece near where speed puts it, nudged to fit.
#[derive(Debug, Clone)]
pub struct Stretch {
    channels: usize,
    rate: f64,
    /// Frames per piece; output moves on half of it a step.
    window: usize,
    hop: usize,
    tolerance: usize,
    /// The rising and falling halves' weights, summing to one.
    weights: Vec<f32>,
    /// Input not yet used, interleaved.
    input: Vec<f32>,
    /// Frames dropped from the front of `input` since the start.
    dropped: usize,
    /// Where speed puts the next piece, in frames since the start.
    ideal: f64,
    /// Where the last piece would have gone on: the shape to match.
    natural: Option<usize>,
    /// The last piece's second half, waiting to overlap the next.
    tail: Vec<f32>,
}

impl Stretch {
    /// For audio at `sample_rate` in `channels`, played at `rate` speed.
    pub fn new(sample_rate: u32, channels: usize, rate: f32) -> Self {
        let sr = f64::from(sample_rate.max(1));
        let hop = ((WINDOW * sr) as usize / 2).max(16);
        let window = hop * 2;
        let weights = (0..window)
            .map(|i| {
                let x = std::f64::consts::PI * i as f64 / window as f64;
                x.sin().powi(2) as f32
            })
            .collect();
        let channels = channels.max(1);
        Self {
            channels,
            rate: f64::from(rate).clamp(0.25, 4.0),
            window,
            hop,
            tolerance: (TOLERANCE * sr) as usize,
            weights,
            input: Vec::new(),
            dropped: 0,
            ideal: 0.0,
            natural: None,
            tail: vec![0.0; hop * channels],
        }
    }

    /// Input frames, since the start, that the next output frame plays.
    pub(crate) fn position(&self) -> f64 {
        self.ideal
    }

    /// Takes `input` and returns the output it completes.
    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        self.input.extend_from_slice(input);
        let mut out = Vec::new();
        while let Some(piece) = self.next_piece() {
            out.extend(piece);
        }
        self.trim();
        out
    }

    /// What is left: the last piece's fading half.
    pub fn finish(&mut self) -> Vec<f32> {
        let mut out = std::mem::take(&mut self.tail);
        if self.natural.is_none() {
            out.clear();
        }
        out
    }

    /// Plays out all input, silence after it, then the fading half.
    pub fn drain(&mut self) -> Vec<f32> {
        // Enough silence that the last piece passes the input's end.
        let reach = self.rate.max(1.0 / self.rate);
        let pad = (self.window + 2 * self.tolerance) as f64 * reach;
        let mut out = self.push(&vec![0.0; pad.ceil() as usize * self.channels]);
        out.extend(self.finish());
        out
    }

    /// One step's output, or `None` until more input comes.
    fn next_piece(&mut self) -> Option<Vec<f32>> {
        let ch = self.channels;
        let have = self.input.len() / ch;
        let target = (self.ideal.round() as usize).saturating_sub(self.dropped);
        let low = target.saturating_sub(self.tolerance);
        let high = target + self.tolerance;
        if high + self.window > have {
            return None;
        }
        let chosen = match self.natural {
            Some(natural) => self.best(natural.saturating_sub(self.dropped), low..=high),
            None => target,
        };
        let piece = self.input.get(chosen * ch..(chosen + self.window) * ch)?;
        let mut out = Vec::with_capacity(self.hop * ch);
        let (first, second) = piece.split_at(self.hop * ch);
        let (rise, fall) = self.weights.split_at(self.hop);
        for ((frame, tail), w) in first
            .chunks_exact(ch)
            .zip(self.tail.chunks_exact(ch))
            .zip(rise)
        {
            out.extend(frame.iter().zip(tail).map(|(s, t)| s * w + t));
        }
        let mut tail = Vec::with_capacity(self.hop * ch);
        for (frame, w) in second.chunks_exact(ch).zip(fall) {
            tail.extend(frame.iter().map(|s| s * w));
        }
        self.tail = tail;
        self.natural = Some(self.dropped + chosen + self.hop);
        self.ideal += self.hop as f64 * self.rate;
        Some(out)
    }

    /// The start in `range` whose first half best matches `natural`'s.
    fn best(&self, natural: usize, range: std::ops::RangeInclusive<usize>) -> usize {
        let start = *range.start();
        let mono = |at: usize| -> Vec<f32> {
            let ch = self.channels;
            let span = self
                .input
                .get(at * ch..(at + self.hop) * ch)
                .unwrap_or_default();
            // Every other frame is plenty to line up speech.
            span.chunks_exact(ch)
                .step_by(2)
                .map(|f| f.iter().sum())
                .collect()
        };
        let shape = mono(natural);
        if shape.is_empty() {
            return start;
        }
        let mut best = (f32::MIN, start);
        for at in range.step_by(2) {
            let other = mono(at);
            let (dot, energy) = (shape.iter().zip(&other))
                .fold((0.0, 0.0), |(d, e), (a, b)| (d + a * b, e + b * b));
            let score = dot / energy.max(1e-9).sqrt();
            if score > best.0 {
                best = (score, at);
            }
        }
        best.1
    }

    /// Drops input no later piece can reach.
    fn trim(&mut self) {
        let target = (self.ideal.round() as usize).saturating_sub(self.dropped);
        let natural = self
            .natural
            .map_or(target, |n| n.saturating_sub(self.dropped));
        let keep = target.saturating_sub(self.tolerance).min(natural);
        let frames = keep.min(self.input.len() / self.channels);
        if frames > 0 {
            self.input.drain(..frames * self.channels);
            self.dropped += frames;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f64, hz: f64) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn stretched(rate: f32, input: &[f32]) -> Vec<f32> {
        let mut s = Stretch::new(8000, 1, rate);
        let mut out = Vec::new();
        for chunk in input.chunks(500) {
            out.extend(s.push(chunk));
        }
        out.extend(s.finish());
        out
    }

    /// Zero crossings a second: twice the pitch.
    fn crossings(samples: &[f32], rate: f64) -> f64 {
        let n = samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        n as f64 / (samples.len() as f64 / rate)
    }

    #[test]
    fn double_speed_halves_the_length_at_the_same_pitch() {
        let input = tone(8000, 4.0, 200.0);
        let out = stretched(2.0, &input);
        let ratio = out.len() as f64 / input.len() as f64;
        assert!((0.47..0.53).contains(&ratio), "{ratio}");
        let pitch = crossings(&out[400..out.len() - 400], 8000.0) / 2.0;
        assert!((190.0..210.0).contains(&pitch), "{pitch}");
    }

    #[test]
    fn slower_speech_lasts_longer() {
        let input = tone(8000, 2.0, 150.0);
        let out = stretched(0.75, &input);
        let ratio = out.len() as f64 / input.len() as f64;
        assert!((1.28..1.38).contains(&ratio), "{ratio}");
        // Pieces line up: no sample leaps out of the wave.
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 1.1, "{peak}");
    }

    #[test]
    fn a_drained_stretch_keeps_the_input_to_its_end() {
        let input = tone(8000, 0.3, 200.0);
        for rate in [0.5, 1.6, 2.5] {
            let mut s = Stretch::new(8000, 1, rate);
            let mut out = s.push(&input);
            out.extend(s.drain());
            let wanted = input.len() as f64 / f64::from(rate);
            let ratio = out.len() as f64 / wanted;
            assert!((1.0..1.6).contains(&ratio), "{rate}: {ratio}");
            // The tone's last stretch is still heard.
            let near = |part: f64| (wanted * part) as usize;
            let last = out.get(near(0.9)..near(0.95)).unwrap();
            assert!(last.iter().any(|s| s.abs() > 0.5), "{rate}");
        }
    }

    #[test]
    fn its_position_follows_the_input_at_speed() {
        let mut s = Stretch::new(8000, 2, 1.5);
        let out = s.push(&vec![0.0; 2 * 8000]);
        let played = out.len() / 2;
        // Output frames times the speed is how far into the input it is.
        assert!((s.position() - played as f64 * 1.5).abs() < 1.0);
    }
}
