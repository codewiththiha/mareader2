//! The system's sound output, fed from a thread of its own.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::feed::Feed;
use crate::{Command, Fault, Fetch, Shared, lock};

/// How much sound is decoded ahead of what is heard, in seconds.
const AHEAD: f64 = 0.25;
/// How often the thread looks in while sound plays.
const TICK: Duration = Duration::from_millis(10);

/// Starts the thread; it ends when the returned sender is dropped.
pub(crate) fn spawn(fetch: Fetch, shared: Arc<Mutex<Shared>>) -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("narration".into())
        .spawn(move || run(&fetch, &shared, &rx));
    if spawned.is_err() {
        // `shared` went with the closure; the sender just goes nowhere.
        drop(tx);
        return mpsc::channel().0;
    }
    tx
}

/// An open output and the feed that fills it.
struct Output {
    /// Held so the sound keeps going; dropping it stops the device.
    _stream: cpal::Stream,
    feed: Feed,
    rate: u32,
}

fn run(fetch: &Fetch, shared: &Arc<Mutex<Shared>>, rx: &Receiver<Command>) {
    let mut output: Option<Output> = None;
    let mut generation = 0;
    loop {
        let busy = output.as_ref().is_some_and(|o| !o.feed.ended());
        let command = match busy {
            true => rx.recv_timeout(TICK),
            false => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match command {
            Ok(Command::Play(segments, current)) => {
                generation = current;
                if output.is_none() {
                    output = open(fetch, shared);
                }
                match output.as_mut() {
                    Some(o) => o.feed.play(segments),
                    None => lock(shared).fault = Some(Fault::NoDevice),
                }
            }
            Ok(Command::Queue(segments, current)) => {
                if let Some(o) = output.as_mut().filter(|_| current == generation) {
                    o.feed.queue(segments);
                    lock(shared).ring.reopen();
                }
            }
            Ok(Command::Rate(rate)) => {
                if let Some(o) = output.as_mut() {
                    let mut s = lock(shared);
                    let heard = s.ring.now();
                    s.ring.clear();
                    o.feed.set_speed(rate, heard);
                }
            }
            Ok(Command::Stop) => {
                if let Some(o) = output.as_mut() {
                    o.feed.stop();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some(o) = output.as_mut() {
            fill(o, shared, generation);
        }
    }
}

/// Tops the ring up, unless a newer play has come in meanwhile.
fn fill(o: &mut Output, shared: &Mutex<Shared>, generation: u64) {
    let ahead = (AHEAD * f64::from(o.rate)) as usize;
    let wanted = {
        let s = lock(shared);
        if s.generation != generation || s.ring.ended() {
            return;
        }
        ahead.saturating_sub(s.ring.frames())
    };
    // Decoding happens unlocked, so the device never waits on it.
    let batch = o.feed.batch(wanted);
    let mut s = lock(shared);
    if s.generation == generation {
        s.ring.accept(batch);
    }
    if let Some(fault) = o.feed.fault.take() {
        s.fault = Some(fault);
    }
}

/// The default output, playing what lands in `shared`.
fn open(fetch: &Fetch, shared: &Arc<Mutex<Shared>>) -> Option<Output> {
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let (rate, channels) = (config.sample_rate, usize::from(config.channels));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, config, shared),
        SampleFormat::I16 => stream::<i16>(&device, config, shared),
        SampleFormat::U16 => stream::<u16>(&device, config, shared),
        SampleFormat::I32 => stream::<i32>(&device, config, shared),
        _ => None,
    }?;
    stream.play().ok()?;
    Some(Output {
        _stream: stream,
        feed: Feed::new(Arc::clone(fetch), rate, channels),
        rate,
    })
}

fn stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: &Arc<Mutex<Shared>>,
) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let shared = Arc::clone(shared);
    let mut scratch: Vec<f32> = Vec::new();
    let data = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        scratch.resize(out.len(), 0.0);
        // Never wait on the reader's thread here: a busy lock is a hush.
        match shared.try_lock() {
            Ok(mut s) => s.take(&mut scratch),
            Err(_) => scratch.fill(0.0),
        }
        for (slot, s) in out.iter_mut().zip(&scratch) {
            *slot = T::from_sample(*s);
        }
    };
    // A lost device shows as silence; the reader can stop and start again.
    let error = |_| {};
    device.build_output_stream(config, data, error, None).ok()
}
