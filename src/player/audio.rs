use crate::player::timeline::{DeviceFormat, Timeline};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where decoded audio goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    /// Default system output device. Falls back to no audio if unavailable.
    Device,
    /// Consumes audio in real time and discards it (headless / tests).
    Null { rate: u32, channels: usize },
    /// No audio at all; playback is timed by the wall clock.
    Disabled,
}

/// Keeps the audio sink alive. Must stay on the thread that created it.
pub enum AudioOutput {
    Device(cpal::Stream),
    Null(Arc<AtomicBool>),
    Disabled,
}

impl AudioOutput {
    pub fn open(backend: AudioBackend, timeline: &Arc<Timeline>) -> Self {
        match backend {
            AudioBackend::Device => match open_device(timeline) {
                Ok(stream) => Self::Device(stream),
                Err(err) => {
                    eprintln!("Audio output unavailable, playing without sound: {err}");
                    Self::Disabled
                }
            },
            AudioBackend::Null { rate, channels } => {
                timeline.attach_device(DeviceFormat { rate, channels });
                let stop = Arc::new(AtomicBool::new(false));
                spawn_null_device(timeline.clone(), rate, channels, stop.clone());
                Self::Null(stop)
            }
            AudioBackend::Disabled => Self::Disabled,
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        if let Self::Null(stop) = self {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

fn open_device(timeline: &Arc<Timeline>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let format = DeviceFormat {
        rate: config.sample_rate,
        channels: config.channels as usize,
    };

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, timeline),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, timeline),
        cpal::SampleFormat::I32 => build::<i32>(&device, config, timeline),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, timeline),
        other => Err(format!("unsupported sample format {other}")),
    }?;

    // Only now does the device drive the clock.
    timeline.attach_device(format);
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    timeline: &Arc<Timeline>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let timeline = timeline.clone();
    let mut scratch: Vec<f32> = Vec::new();

    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(ts.callback).as_secs_f64();

                scratch.resize(data.len(), 0.0);
                timeline.fill(&mut scratch, latency);
                for (dst, src) in data.iter_mut().zip(&scratch) {
                    *dst = T::from_sample(*src);
                }
            },
            |err| eprintln!("Audio stream error: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn spawn_null_device(timeline: Arc<Timeline>, rate: u32, channels: usize, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        const PERIOD: Duration = Duration::from_millis(10);
        let mut buf = vec![0.0f32; (rate as usize / 100) * channels];
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            timeline.fill(&mut buf, 0.0);
            next += PERIOD;
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
    });
}
