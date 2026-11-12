use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

/// Audio whose PTS is off by more than this from where the buffer says it
/// should land gets trimmed / padded with silence to restore A/V sync.
const SYNC_TOLERANCE: f64 = 0.03;
/// Upper bound for silence inserted to bridge a gap in the audio stream.
const MAX_GAP_SECS: f64 = 10.0;
/// The clock is extrapolated between audio callbacks, but never further than this.
const MAX_EXTRAPOLATION: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceFormat {
    pub rate: u32,
    pub channels: usize,
}

/// Master playback clock plus the audio sample buffer it is derived from.
///
/// With an audio device attached, time advances by the number of frames the
/// device has consumed, so video stays locked to what is actually heard.
/// Without one, it falls back to the wall clock.
pub struct Timeline {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Media time the counters below are relative to (last load / seek).
    base_pts: f64,
    playing: bool,
    /// Clock frozen until the first frame after a load / seek is ready.
    holding: bool,

    device: Option<DeviceFormat>,
    ring: VecDeque<f32>,
    /// Frames handed to the device since `base_pts` (silence included).
    played_frames: u64,
    /// Frames placed on the timeline since `base_pts`: buffered + played.
    written_frames: u64,
    /// (when, media time) of the most recent device callback.
    anchor: Option<(Instant, f64)>,
    latency: f64,
    /// Real (non-silence) samples handed to the device, ever.
    delivered_samples: u64,

    wall_started: Option<Instant>,
    wall_accum: f64,
}

impl Inner {
    fn running(&self) -> bool {
        self.playing && !self.holding
    }

    fn sync_wall(&mut self) {
        if self.running() {
            if self.wall_started.is_none() {
                self.wall_started = Some(Instant::now());
            }
        } else if let Some(started) = self.wall_started.take() {
            self.wall_accum += started.elapsed().as_secs_f64();
        }
        self.anchor = None;
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                base_pts: 0.0,
                playing: false,
                holding: false,
                device: None,
                ring: VecDeque::new(),
                played_frames: 0,
                written_frames: 0,
                anchor: None,
                latency: 0.0,
                delivered_samples: 0,
                wall_started: None,
                wall_accum: 0.0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Switches the clock from wall time to the given audio device.
    pub fn attach_device(&self, format: DeviceFormat) {
        self.lock().device = Some(format);
    }

    pub fn device_format(&self) -> Option<DeviceFormat> {
        self.lock().device
    }

    pub fn is_playing(&self) -> bool {
        self.lock().playing
    }

    pub fn set_playing(&self, playing: bool) {
        let mut s = self.lock();
        s.playing = playing;
        s.sync_wall();
    }

    /// Jumps to `pts`, dropping buffered audio. The clock stays frozen until
    /// [`release`](Self::release) so playback resumes in sync.
    pub fn reset(&self, pts: f64) {
        let mut s = self.lock();
        s.base_pts = pts;
        s.ring.clear();
        s.played_frames = 0;
        s.written_frames = 0;
        s.wall_started = None;
        s.wall_accum = 0.0;
        s.holding = true;
        s.sync_wall();
    }

    /// Lets the clock run again after a [`reset`](Self::reset).
    pub fn release(&self) {
        let mut s = self.lock();
        if s.holding {
            s.holding = false;
            s.sync_wall();
        }
    }

    /// Current media time in seconds.
    pub fn position(&self) -> f64 {
        let s = self.lock();
        match s.device {
            Some(dev) => {
                let pos = match s.anchor {
                    Some((at, pts)) if s.running() => {
                        pts + at.elapsed().as_secs_f64().min(MAX_EXTRAPOLATION)
                    }
                    _ => s.base_pts + s.played_frames as f64 / f64::from(dev.rate) - s.latency,
                };
                pos.max(s.base_pts)
            }
            None => {
                let running = s.wall_started.map_or(0.0, |t| t.elapsed().as_secs_f64());
                s.base_pts + s.wall_accum + running
            }
        }
    }

    /// Seconds of audio waiting to be played.
    pub fn buffered_secs(&self) -> f64 {
        let s = self.lock();
        match s.device {
            Some(dev) => s.ring.len() as f64 / dev.channels as f64 / f64::from(dev.rate),
            None => 0.0,
        }
    }

    pub fn delivered_samples(&self) -> u64 {
        self.lock().delivered_samples
    }

    /// Queues interleaved samples (device rate / channels) that start at `pts`.
    ///
    /// Samples that belong before the current write position are dropped and
    /// gaps are filled with silence, so audio always lands at its own PTS.
    pub fn push_audio(&self, pts: f64, samples: &[f32]) {
        let mut s = self.lock();
        let Some(dev) = s.device else { return };
        let rate = f64::from(dev.rate);
        let frames = samples.len() / dev.channels;

        let expected = s.base_pts + s.written_frames as f64 / rate;
        let diff = pts - expected;

        let mut skip = 0;
        if diff < -SYNC_TOLERANCE {
            skip = ((-diff * rate).round() as usize).min(frames);
        } else if diff > SYNC_TOLERANCE {
            let gap = (diff.min(MAX_GAP_SECS) * rate).round() as usize;
            let new_len = s.ring.len() + gap * dev.channels;
            s.ring.resize(new_len, 0.0);
            s.written_frames += gap as u64;
        }

        s.ring.extend(&samples[skip * dev.channels..frames * dev.channels]);
        s.written_frames += (frames - skip) as u64;
    }

    /// Audio device callback: fills `out` and advances the clock.
    /// `latency` is how long until these samples are actually heard.
    pub fn fill(&self, out: &mut [f32], latency: f64) {
        let mut s = self.lock();
        let Some(dev) = s.device else {
            out.fill(0.0);
            return;
        };
        if !s.running() {
            out.fill(0.0);
            s.anchor = None;
            return;
        }

        let n = out.len().min(s.ring.len());
        for (dst, src) in out[..n].iter_mut().zip(s.ring.drain(..n)) {
            *dst = src;
        }
        out[n..].fill(0.0);

        let rate = f64::from(dev.rate);
        s.latency = latency.clamp(0.0, 0.5);
        let at_callback = s.base_pts + s.played_frames as f64 / rate - s.latency;
        s.anchor = Some((Instant::now(), at_callback));

        // Silence played on underrun still occupies timeline, so late audio
        // gets trimmed by `push_audio` instead of drifting out of sync.
        s.played_frames += (out.len() / dev.channels) as u64;
        s.written_frames += ((out.len() - n) / dev.channels) as u64;
        s.delivered_samples += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_48k() -> Timeline {
        let t = Timeline::new();
        t.attach_device(DeviceFormat { rate: 48_000, channels: 2 });
        t
    }

    #[test]
    fn device_clock_advances_only_while_playing() {
        let t = stereo_48k();
        t.push_audio(0.0, &vec![0.5; 9600]); // 0.1s
        let mut out = vec![0.0; 960];

        t.fill(&mut out, 0.0);
        assert!(out.iter().all(|s| *s == 0.0), "paused must output silence");
        assert_eq!(t.position(), 0.0);

        t.set_playing(true);
        for _ in 0..5 {
            t.fill(&mut out, 0.0);
        }
        assert!(out.iter().all(|s| *s == 0.5));
        t.set_playing(false);
        assert!((t.position() - 0.05).abs() < 1e-9, "{}", t.position());
        assert!((t.buffered_secs() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn audio_before_seek_target_is_trimmed() {
        let t = stereo_48k();
        t.reset(10.0);
        // Chunk starts 0.05s before the target: first half must be dropped.
        t.push_audio(9.95, &vec![1.0; 9600]);
        assert!((t.buffered_secs() - 0.05).abs() < 1e-9, "{}", t.buffered_secs());
        // Entirely before target: dropped completely.
        let t = stereo_48k();
        t.reset(10.0);
        t.push_audio(9.0, &vec![1.0; 9600]);
        assert_eq!(t.buffered_secs(), 0.0);
    }

    #[test]
    fn gap_is_filled_with_silence() {
        let t = stereo_48k();
        t.push_audio(0.5, &vec![1.0; 960]);
        assert!((t.buffered_secs() - 0.51).abs() < 1e-9);
    }

    #[test]
    fn underrun_keeps_late_audio_in_sync() {
        let t = stereo_48k();
        t.release();
        t.set_playing(true);
        let mut out = vec![0.0; 9600]; // 0.1s of underrun
        t.fill(&mut out, 0.0);
        // Audio for 0.0..0.2 arrives late: the first 0.1s is already past.
        t.push_audio(0.0, &vec![1.0; 19200]);
        assert!((t.buffered_secs() - 0.1).abs() < 1e-9, "{}", t.buffered_secs());
    }

    #[test]
    fn clock_is_frozen_while_holding() {
        let t = Timeline::new();
        t.set_playing(true);
        t.reset(3.0);
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(t.position(), 3.0);
        t.release();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(t.position() > 3.02);
    }
}
