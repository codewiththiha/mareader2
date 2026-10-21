//! Samples brought to the device's rate and channels as they stream.

/// Straight lines between samples, which speech forgives.
#[derive(Debug, Clone)]
pub(crate) struct Resampler {
    channels: usize,
    /// The last frame of the stream so far, mapped.
    last: Option<Vec<f32>>,
    /// Where the next output falls, from the next chunk's first frame.
    pos: f64,
}

impl Resampler {
    /// Output in `channels` channels.
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            last: None,
            pos: 0.0,
        }
    }

    /// `input`, `from` channels at `from_rate`, as frames at `to_rate`.
    pub(crate) fn push(
        &mut self,
        input: &[f32],
        from: usize,
        from_rate: u32,
        to_rate: u32,
    ) -> Vec<f32> {
        let mapped = mapped(input, from, self.channels);
        if from_rate == to_rate || from_rate == 0 || to_rate == 0 {
            self.last = mapped
                .rchunks_exact(self.channels)
                .next()
                .map(<[f32]>::to_vec);
            return mapped;
        }
        let ch = self.channels;
        let frames = mapped.len() / ch;
        let step = f64::from(from_rate) / f64::from(to_rate);
        // The first chunk starts at its first frame, with nothing before.
        let last = match self.last.take() {
            Some(l) => l,
            None => match mapped.get(..ch) {
                Some(first) => first.to_vec(),
                None => return Vec::new(),
            },
        };
        let frame = |k: isize| -> &[f32] {
            match usize::try_from(k) {
                Ok(k) => mapped.get(k * ch..(k + 1) * ch).unwrap_or(&last),
                Err(_) => &last,
            }
        };
        let mut out = Vec::with_capacity((frames as f64 / step) as usize * ch + ch);
        let mut t = self.pos;
        let end = frames as f64 - 1.0;
        while t < end {
            let below = t.floor();
            let k = below as isize;
            let w = (t - below) as f32;
            let (a, b) = (frame(k), frame(k + 1));
            out.extend(a.iter().zip(b).map(|(a, b)| a + (b - a) * w));
            t += step;
        }
        self.pos = t - frames as f64;
        self.last = mapped
            .rchunks_exact(ch)
            .next()
            .map(<[f32]>::to_vec)
            .or(Some(last));
        out
    }
}

/// Frames of `from` channels as `to`: one spread to all, many folded.
fn mapped(input: &[f32], from: usize, to: usize) -> Vec<f32> {
    let from = from.max(1);
    if from == to {
        return input.to_vec();
    }
    let mut out = Vec::with_capacity(input.len() / from * to);
    for frame in input.chunks_exact(from) {
        match (from, to) {
            (1, _) => out.extend(std::iter::repeat_n(
                frame.first().copied().unwrap_or(0.0),
                to,
            )),
            (_, 1) => out.push(frame.iter().sum::<f32>() / from as f32),
            // Front left and right lead; extra outputs repeat them.
            _ => out.extend((0..to).map(|c| frame.get(c % from.min(2)).copied().unwrap_or(0.0))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_kept_passes_untouched() {
        let mut r = Resampler::new(2);
        let input = [0.1, 0.2, 0.3, 0.4];
        assert_eq!(r.push(&input, 2, 44100, 44100), input);
    }

    #[test]
    fn doubling_the_rate_draws_the_line_between() {
        let mut r = Resampler::new(1);
        let out = r.push(&[0.0, 1.0, 2.0, 3.0], 1, 1, 2);
        assert_eq!(out, [0.0, 0.5, 1.0, 1.5, 2.0, 2.5]);
        // The next chunk joins on with no seam.
        let more = r.push(&[4.0, 5.0], 1, 1, 2);
        assert_eq!(more, [3.0, 3.5, 4.0, 4.5]);
    }

    #[test]
    fn a_long_stream_keeps_its_length_in_time() {
        let mut r = Resampler::new(1);
        let mut total = 0;
        for _ in 0..100 {
            total += r.push(&[0.5; 441], 1, 44100, 48000).len();
        }
        // 44100 frames are a second: 48000 at the new rate, give or take.
        assert!((47_990..=48_010).contains(&total), "{total}");
    }

    #[test]
    fn channels_spread_and_fold() {
        assert_eq!(mapped(&[0.5, 0.25], 1, 2), [0.5, 0.5, 0.25, 0.25]);
        assert_eq!(mapped(&[0.2, 0.4], 2, 1), [0.3f32]);
        assert_eq!(mapped(&[0.1, 0.2, 0.3], 3, 2), [0.1, 0.2]);
    }
}
