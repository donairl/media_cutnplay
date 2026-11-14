//! Whole-file transcoding: re-encode the video, re-encode or copy the audio.

use crate::core::error::MediaError;
use ffmpeg_next::{
    codec, decoder, encoder, filter, format, frame, media, picture, ChannelLayout, Dictionary,
    Packet, Rational, Rescale,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
}

impl VideoCodec {
    /// How far this codec's quality scale sits above H.264's: the level
    /// that gives about the same picture is this much higher.
    fn crf_offset(self) -> u8 {
        match self {
            Self::H264 => 0,
            Self::Hevc => 4,
            Self::Vp9 => 8,
            Self::Av1 => 10,
        }
    }

    /// Quality levels the slider offers. Lower is better and bigger;
    /// roughly every +6 halves the file.
    pub fn crf_range(self) -> std::ops::RangeInclusive<u8> {
        (14 + self.crf_offset())..=(36 + self.crf_offset())
    }

    /// Whether NVIDIA GPUs have an encoder for this codec at all; whether
    /// this machine's does is [`NvencSupport`].
    pub fn has_nvenc(self) -> bool {
        self != Self::Vp9
    }
}

/// Which codecs this machine's NVIDIA GPU can encode. Older cards lack the
/// newer ones (AV1 needs an RTX 40-class GPU), and a machine without an
/// NVIDIA GPU supports none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NvencSupport {
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
}

impl NvencSupport {
    pub fn supports(self, codec: VideoCodec) -> bool {
        match codec {
            VideoCodec::H264 => self.h264,
            VideoCodec::Hevc => self.hevc,
            VideoCodec::Av1 => self.av1,
            VideoCodec::Vp9 => false,
        }
    }

    /// Asks the GPU by opening each encoder once. Takes a moment (driver
    /// start-up), so call it off the UI thread.
    pub fn probe() -> Self {
        // A refused encoder is the expected answer here, not an error to print.
        let level = ffmpeg_next::util::log::get_level().ok();
        ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Quiet);
        let support = Self {
            h264: encoder_opens("h264_nvenc"),
            hevc: encoder_opens("hevc_nvenc"),
            av1: encoder_opens("av1_nvenc"),
        };
        if let Some(level) = level {
            ffmpeg_next::util::log::set_level(level);
        }
        support
    }
}

/// Whether the named video encoder starts for an ordinary 720p stream.
fn encoder_opens(name: &str) -> bool {
    let Some(codec) = encoder::find_by_name(name) else {
        return false;
    };
    let Ok(mut encoder) = codec::context::Context::new_with_codec(codec).encoder().video() else {
        return false;
    };
    encoder.set_width(1280);
    encoder.set_height(720);
    encoder.set_format(format::Pixel::YUV420P);
    encoder.set_time_base(Rational::new(1, 30));
    encoder.set_frame_rate(Some(Rational::new(30, 1)));
    encoder.open().is_ok()
}

/// One-click quality levels for the constant-quality mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    /// Hard to tell from the source; large file.
    High,
    /// The encoders' usual default.
    Medium,
    /// Visibly softer; small file.
    Low,
}

impl QualityPreset {
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    /// The level on H.264's scale; other codecs add their offset.
    fn h264_crf(self) -> u8 {
        match self {
            Self::High => 18,
            Self::Medium => 23,
            Self::Low => 28,
        }
    }

    /// The quality level this preset means for `codec`.
    pub fn crf(self, codec: VideoCodec) -> u8 {
        self.h264_crf() + codec.crf_offset()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    /// libx264 / libx265 on the CPU.
    Software,
    /// NVIDIA GPU encoder.
    Nvenc,
}

/// What steers the video encoder's size/quality trade-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateMode {
    /// Constant quality (CRF, or CQ on NVENC): the picture stays equally
    /// good and the bitrate follows the content.
    Quality,
    /// Average bitrate: the size is predictable and the quality follows.
    Bitrate,
}

/// Allowed target video bitrates, in kbit/s.
pub const BITRATE_RANGE: std::ops::RangeInclusive<u32> = 250..=40_000;
pub const BITRATE_STEP: u32 = 250;
/// Maximum distance between keyframes in transcoded video.
const KEYFRAME_INTERVAL_SECS: f64 = 5.0;
/// Output height cap. Video is only ever scaled down, keeping its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Source,
    P1080,
    P720,
    P480,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    /// Re-encode every audio track to AAC.
    Aac,
    /// Keep the audio tracks as they are.
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Mkv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscodeSettings {
    pub codec: VideoCodec,
    pub backend: EncoderBackend,
    pub rate_mode: RateMode,
    /// Constant-quality level, used in [`RateMode::Quality`].
    pub crf: u8,
    /// Target video bitrate in kbit/s, used in [`RateMode::Bitrate`].
    pub bitrate_kbps: u32,
    pub resolution: Resolution,
    pub audio: AudioMode,
    pub container: Container,
}

impl Default for TranscodeSettings {
    fn default() -> Self {
        Self {
            codec: VideoCodec::H264,
            backend: EncoderBackend::Software,
            rate_mode: RateMode::Quality,
            crf: 23,
            bitrate_kbps: 5_000,
            resolution: Resolution::Source,
            audio: AudioMode::Aac,
            container: Container::Mp4,
        }
    }
}

impl TranscodeSettings {
    /// File extension of the chosen container.
    pub fn extension(&self) -> &'static str {
        match self.container {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
        }
    }

    /// FFmpeg name of the video encoder these settings select.
    pub fn encoder_name(&self) -> &'static str {
        match (self.codec, self.backend) {
            (VideoCodec::H264, EncoderBackend::Software) => "libx264",
            (VideoCodec::Hevc, EncoderBackend::Software) => "libx265",
            (VideoCodec::Av1, EncoderBackend::Software) => "libsvtav1",
            (VideoCodec::H264, EncoderBackend::Nvenc) => "h264_nvenc",
            (VideoCodec::Hevc, EncoderBackend::Nvenc) => "hevc_nvenc",
            (VideoCodec::Av1, EncoderBackend::Nvenc) => "av1_nvenc",
            // There is no VP9 on NVENC.
            (VideoCodec::Vp9, _) => "libvpx-vp9",
        }
    }

    /// The same settings for another codec, with the quality level moved
    /// so the picture stays about as good, and the encoder switched to the
    /// CPU when the GPU has none for it.
    pub fn with_codec(mut self, codec: VideoCodec) -> Self {
        let level = i16::from(self.crf) - i16::from(self.codec.crf_offset())
            + i16::from(codec.crf_offset());
        let range = codec.crf_range();
        self.crf = level.clamp(i16::from(*range.start()), i16::from(*range.end())) as u8;
        self.codec = codec;
        if !codec.has_nvenc() {
            self.backend = EncoderBackend::Software;
        }
        self
    }

    /// The same settings, on the CPU encoder when this machine's GPU cannot
    /// encode the chosen codec.
    pub fn limited_to(mut self, nvenc: NvencSupport) -> Self {
        if !nvenc.supports(self.codec) {
            self.backend = EncoderBackend::Software;
        }
        self
    }

    fn encoder_options(&self) -> Dictionary<'static> {
        let name = self.encoder_name();
        let nvenc = name.ends_with("_nvenc");
        let mut options = Dictionary::new();

        // Speed presets: each encoder's usual middle ground.
        match name {
            "libx264" => options.set("preset", "medium"),
            "libx265" => {
                options.set("preset", "medium");
                options.set("x265-params", "log-level=error");
            }
            "libsvtav1" => options.set("preset", "8"),
            "libvpx-vp9" => {
                options.set("deadline", "good");
                options.set("cpu-used", "4");
                options.set("row-mt", "1");
            }
            _ => {
                options.set("preset", "p5");
                options.set("rc", "vbr");
            }
        }

        match self.rate_mode {
            RateMode::Quality => {
                let range = self.codec.crf_range();
                let level = self.crf.clamp(*range.start(), *range.end()).to_string();
                if nvenc {
                    options.set("cq", &level);
                } else {
                    options.set("crf", &level);
                }
                // NVENC and VP9 treat a bitrate as a ceiling on the quality
                // level; zero leaves the level alone in charge.
                if nvenc || name == "libvpx-vp9" {
                    options.set("b", "0");
                }
            }
            RateMode::Bitrate => {
                let kbps = self.bitrate_kbps.clamp(*BITRATE_RANGE.start(), *BITRATE_RANGE.end());
                options.set("b", &format!("{kbps}k"));
                // Cap the peaks so hard scenes do not balloon far past the
                // average. SVT-AV1 and libvpx manage that on their own.
                if nvenc || name == "libx264" || name == "libx265" {
                    options.set("maxrate", &format!("{}k", kbps * 3 / 2));
                    options.set("bufsize", &format!("{}k", kbps * 2));
                }
            }
        }
        options
    }
}

impl Resolution {
    fn max_height(self) -> Option<u32> {
        match self {
            Self::Source => None,
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
            Self::P480 => Some(480),
        }
    }

    /// Output size for a source of `width` x `height`: capped height, same
    /// shape, even dimensions (4:2:0 video cannot have odd ones).
    pub fn output_size(self, width: u32, height: u32) -> (u32, u32) {
        let even = |v: u32| (v.max(2) / 2) * 2;
        match self.max_height() {
            Some(cap) if height > cap => {
                let scaled = f64::from(width) * f64::from(cap) / f64::from(height);
                // Nearest even width, as rounding down would drift further
                // from the source's shape.
                ((scaled / 2.0).round().max(1.0) as u32 * 2, even(cap))
            }
            _ => (even(width), even(height)),
        }
    }
}

/// Transcodes `input_path` into `output_path`, whose extension picks the
/// container. Setting `cancel` stops the job; a cancelled or failed job
/// leaves no output file behind.
pub fn transcode<F>(
    input_path: &Path,
    output_path: &Path,
    settings: &TranscodeSettings,
    cancel: &AtomicBool,
    on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    let result = run(input_path, output_path, settings, cancel, on_progress);
    if result.is_err() {
        let _ = std::fs::remove_file(output_path);
    }
    result
}

fn run<F>(
    input_path: &Path,
    output_path: &Path,
    settings: &TranscodeSettings,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    let mut ictx = format::input(input_path)?;
    let mut octx = format::output(output_path)?;

    let main_video = ictx
        .streams()
        .best(media::Type::Video)
        .map(|s| s.index())
        .ok_or(MediaError::NoVideoStream)?;
    // Text subtitles survive a copy into Matroska; MP4 would reject them.
    let keep_subtitles = output_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mkv"));

    let mut pipes: Vec<Option<Pipe>> = Vec::new();
    for (src_idx, istream) in ictx.streams().enumerate() {
        let medium = istream.parameters().medium();
        let pipe = if src_idx == main_video {
            Some(Pipe::Video(VideoPipe::new(&istream, &mut octx, settings)?))
        } else if medium == media::Type::Audio && settings.audio == AudioMode::Aac {
            Some(Pipe::Audio(AudioPipe::new(&istream, &mut octx)?))
        } else if medium == media::Type::Audio
            || (medium == media::Type::Subtitle && keep_subtitles)
        {
            Some(Pipe::Copy(CopyPipe::new(&istream, &mut octx)?))
        } else {
            // Cover art and data streams.
            None
        };
        pipes.push(pipe);
    }

    octx.write_header()?;

    let duration = (ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE)).max(0.001);
    let start = match ictx.stream(main_video) {
        Some(s) if s.start_time() != ffmpeg_next::ffi::AV_NOPTS_VALUE => {
            s.start_time() as f64 * f64::from(s.time_base())
        }
        _ => 0.0,
    };
    let mut reported = 0.0_f32;

    for (stream, packet) in ictx.packets() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let src_idx = stream.index();

        if src_idx == main_video {
            let at = packet.dts().or(packet.pts()).unwrap_or(0) as f64
                * f64::from(stream.time_base());
            let progress = (((at - start) / duration) as f32).clamp(0.0, 0.999);
            // A packet at a time would flood the UI; report in 0.2% steps.
            if progress - reported >= 0.002 {
                reported = progress;
                on_progress(progress);
            }
        }

        match pipes.get_mut(src_idx).and_then(Option::as_mut) {
            Some(Pipe::Video(pipe)) => pipe.push(Some(&packet), &mut octx)?,
            Some(Pipe::Audio(pipe)) => pipe.push(Some(&packet), &mut octx)?,
            Some(Pipe::Copy(pipe)) => pipe.push(packet, &mut octx)?,
            None => {}
        }
    }

    for pipe in pipes.iter_mut().flatten() {
        match pipe {
            Pipe::Video(pipe) => pipe.push(None, &mut octx)?,
            Pipe::Audio(pipe) => pipe.push(None, &mut octx)?,
            Pipe::Copy(_) => {}
        }
    }

    octx.write_trailer()?;
    on_progress(1.0);

    Ok(output_path.to_path_buf())
}

enum Pipe {
    Video(VideoPipe),
    Audio(AudioPipe),
    Copy(CopyPipe),
}

fn failed(what: impl std::fmt::Display) -> MediaError {
    MediaError::TranscodeFailed(what.to_string())
}

/// Writes whatever the encoder has ready, stamped for output stream `out_idx`.
/// Packets the encoder left without a duration get `fallback_duration`
/// ticks, so the last frame is not shown for zero time.
fn drain_encoder(
    encoder: &mut encoder::Encoder,
    time_base: Rational,
    fallback_duration: i64,
    out_idx: usize,
    octx: &mut format::context::Output,
) -> Result<(), MediaError> {
    let mut encoded = Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        if encoded.duration() <= 0 {
            encoded.set_duration(fallback_duration);
        }
        let out_tb = octx.stream(out_idx).map_or(time_base, |s| s.time_base());
        encoded.set_stream(out_idx);
        encoded.rescale_ts(time_base, out_tb);
        encoded.write_interleaved(octx)?;
    }
    Ok(())
}

/// Hands a frame to the graph's input, or signals the end with `None`.
fn feed(graph: &mut filter::Graph, frame: Option<&ffmpeg_next::Frame>) -> Result<(), MediaError> {
    let mut input = graph.get("in").ok_or_else(|| failed("filter graph lost its input"))?;
    match frame {
        Some(frame) => input.source().add(frame)?,
        None => input.source().flush()?,
    }
    Ok(())
}

/// decode -> scale / convert to 8-bit 4:2:0 -> encode
struct VideoPipe {
    out_idx: usize,
    time_base: Rational,
    /// One frame's length in `time_base` ticks, by the average frame rate.
    frame_ticks: i64,
    decoder: decoder::Video,
    graph: filter::Graph,
    encoder: encoder::Video,
}

impl VideoPipe {
    fn new(
        istream: &format::stream::Stream,
        octx: &mut format::context::Output,
        settings: &TranscodeSettings,
    ) -> Result<Self, MediaError> {
        let decoder = codec::context::Context::from_parameters(istream.parameters())?
            .decoder()
            .video()?;
        let (src_w, src_h) = (decoder.width(), decoder.height());
        if src_w == 0 || src_h == 0 || decoder.format() == format::Pixel::None {
            return Err(failed("the video stream does not declare its picture format"));
        }
        let (width, height) = settings.resolution.output_size(src_w, src_h);
        let time_base = istream.time_base();
        let aspect = match decoder.aspect_ratio() {
            ratio if ratio.numerator() > 0 && ratio.denominator() > 0 => ratio,
            _ => Rational::new(1, 1),
        };

        let mut graph = filter::Graph::new();
        let source_args = format!(
            "video_size={src_w}x{src_h}:pix_fmt={}:time_base={}/{}:pixel_aspect={}/{}",
            ffmpeg_next::ffi::AVPixelFormat::from(decoder.format()) as i32,
            time_base.numerator(),
            time_base.denominator(),
            aspect.numerator(),
            aspect.denominator(),
        );
        let find = |name: &str| filter::find(name).ok_or_else(|| failed(format!("FFmpeg lacks the {name} filter")));
        graph.add(&find("buffer")?, "in", &source_args)?;
        graph.add(&find("buffersink")?, "out", "")?;
        graph
            .output("in", 0)?
            .input("out", 0)?
            .parse(&format!("scale={width}:{height}:flags=bicubic,format=yuv420p"))?;
        graph.validate()?;

        let name = settings.encoder_name();
        let codec = encoder::find_by_name(name)
            .ok_or_else(|| failed(format!("this FFmpeg has no {name} encoder")))?;
        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);
        let mut ostream = octx.add_stream(codec)?;
        let out_idx = ostream.index();

        let mut encoder = codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(width);
        encoder.set_height(height);
        encoder.set_format(format::Pixel::YUV420P);
        encoder.set_time_base(time_base);
        let fps = f64::from(istream.avg_frame_rate());
        let frame_ticks = if fps > 0.0 {
            encoder.set_frame_rate(Some(istream.avg_frame_rate()));
            (1.0 / (fps * f64::from(time_base))).round() as i64
        } else {
            0
        };
        // A real keyframe every ~5s so the output stays seekable. Left alone,
        // libsvtav1 gets g=-1 and open-GOP "forward" keyframes, which leaves
        // a single seekable frame for the whole file.
        let fps_or_default = if fps > 0.0 { fps } else { 30.0 };
        encoder.set_gop((fps_or_default * KEYFRAME_INTERVAL_SECS).round().max(1.0) as u32);
        encoder.set_aspect_ratio(aspect);
        encoder.set_colorspace(decoder.color_space());
        encoder.set_color_range(decoder.color_range());
        encoder.set_color_primaries(decoder.color_primaries());
        encoder.set_color_transfer_characteristic(decoder.color_transfer_characteristic());
        let mut flags = codec::Flags::CLOSED_GOP;
        if global_header {
            flags |= codec::Flags::GLOBAL_HEADER;
        }
        encoder.set_flags(flags);

        let encoder = encoder
            .open_with(settings.encoder_options())
            .map_err(|err| failed(format!("could not start the {name} encoder ({err})")))?;
        ostream.set_parameters(&encoder);

        Ok(Self { out_idx, time_base, frame_ticks, decoder, graph, encoder })
    }

    /// Feeds one packet through, or flushes everything on `None`.
    fn push(
        &mut self,
        packet: Option<&Packet>,
        octx: &mut format::context::Output,
    ) -> Result<(), MediaError> {
        match packet {
            // A damaged packet costs its frames, not the whole job.
            Some(packet) => drop(self.decoder.send_packet(packet)),
            None => drop(self.decoder.send_eof()),
        }

        let mut decoded = frame::Video::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let timestamp = decoded.timestamp();
            decoded.set_pts(timestamp);
            feed(&mut self.graph, Some(&decoded))?;
            self.encode_filtered(octx)?;
        }

        if packet.is_none() {
            feed(&mut self.graph, None)?;
            self.encode_filtered(octx)?;
            self.encoder.send_eof()?;
            drain_encoder(&mut self.encoder, self.time_base, self.frame_ticks, self.out_idx, octx)?;
        }
        Ok(())
    }

    fn encode_filtered(&mut self, octx: &mut format::context::Output) -> Result<(), MediaError> {
        let mut filtered = frame::Video::empty();
        loop {
            let mut sink = self.graph.get("out").ok_or_else(|| failed("filter graph lost its output"))?;
            if sink.sink().frame(&mut filtered).is_err() {
                return Ok(());
            }
            // Let the encoder pick its own frame types.
            filtered.set_kind(picture::Type::None);
            // Rate control budgets bits by how long a frame is shown; left
            // blank, libvpx assumes one tick and misses its bitrate badly.
            if self.frame_ticks > 0 {
                // SAFETY: `filtered` holds a valid AVFrame that we own.
                unsafe {
                    (*filtered.as_mut_ptr()).duration = self.frame_ticks;
                }
            }
            self.encoder.send_frame(&filtered)?;
            drain_encoder(&mut self.encoder, self.time_base, self.frame_ticks, self.out_idx, octx)?;
        }
    }
}

/// decode -> resample into AAC-sized frames -> encode
struct AudioPipe {
    out_idx: usize,
    time_base: Rational,
    decoder: decoder::Audio,
    graph: filter::Graph,
    encoder: encoder::Audio,
}

impl AudioPipe {
    fn new(
        istream: &format::stream::Stream,
        octx: &mut format::context::Output,
    ) -> Result<Self, MediaError> {
        let decoder = codec::context::Context::from_parameters(istream.parameters())?
            .decoder()
            .audio()?;
        let rate = decoder.rate();
        if rate == 0 {
            return Err(failed("an audio stream does not declare its sample rate"));
        }
        // Raw formats often leave the layout blank; go by channel count.
        let in_layout = match decoder.channel_layout() {
            layout if layout.bits() != 0 => layout,
            _ => ChannelLayout::default(i32::from(decoder.channels().max(1))),
        };

        let codec = encoder::find(codec::Id::AAC)
            .ok_or_else(|| failed("this FFmpeg has no AAC encoder"))?;
        let audio_codec = codec.audio()?;
        let out_layout = audio_codec
            .channel_layouts()
            .map(|layouts| layouts.best(in_layout.channels()))
            .unwrap_or(ChannelLayout::STEREO);
        let sample_format = audio_codec
            .formats()
            .and_then(|mut formats| formats.next())
            .ok_or_else(|| failed("the AAC encoder lists no sample format"))?;
        let time_base = Rational::new(1, rate as i32);

        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);
        let mut ostream = octx.add_stream(codec)?;
        let out_idx = ostream.index();

        let mut encoder = codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()?;
        encoder.set_rate(rate as i32);
        encoder.set_channel_layout(out_layout);
        encoder.set_format(sample_format);
        encoder.set_bit_rate(if out_layout.channels() > 2 { 384_000 } else { 160_000 });
        encoder.set_time_base(time_base);
        if global_header {
            encoder.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let encoder = encoder
            .open_as(codec)
            .map_err(|err| failed(format!("could not start the AAC encoder ({err})")))?;
        ostream.set_parameters(&encoder);

        let in_tb = istream.time_base();
        let mut graph = filter::Graph::new();
        let source_args = format!(
            "time_base={}/{}:sample_rate={rate}:sample_fmt={}:channel_layout=0x{:x}",
            in_tb.numerator(),
            in_tb.denominator(),
            decoder.format().name(),
            in_layout.bits(),
        );
        let find = |name: &str| filter::find(name).ok_or_else(|| failed(format!("FFmpeg lacks the {name} filter")));
        graph.add(&find("abuffer")?, "in", &source_args)?;
        graph.add(&find("abuffersink")?, "out", "")?;
        // Convert to exactly what the encoder was opened with.
        let conversion = format!(
            "aformat=sample_fmts={}:sample_rates={rate}:channel_layouts=0x{:x}",
            sample_format.name(),
            out_layout.bits(),
        );
        graph.output("in", 0)?.input("out", 0)?.parse(&conversion)?;
        graph.validate()?;
        // AAC takes fixed-size frames; have the sink cut them to measure.
        if encoder.frame_size() > 0 {
            if let Some(mut out) = graph.get("out") {
                out.sink().set_frame_size(encoder.frame_size());
            }
        }

        Ok(Self { out_idx, time_base, decoder, graph, encoder })
    }

    fn push(
        &mut self,
        packet: Option<&Packet>,
        octx: &mut format::context::Output,
    ) -> Result<(), MediaError> {
        match packet {
            Some(packet) => drop(self.decoder.send_packet(packet)),
            None => drop(self.decoder.send_eof()),
        }

        let mut decoded = frame::Audio::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let timestamp = decoded.timestamp();
            decoded.set_pts(timestamp);
            feed(&mut self.graph, Some(&decoded))?;
            self.encode_filtered(octx)?;
        }

        if packet.is_none() {
            feed(&mut self.graph, None)?;
            self.encode_filtered(octx)?;
            self.encoder.send_eof()?;
            drain_encoder(&mut self.encoder, self.time_base, 0, self.out_idx, octx)?;
        }
        Ok(())
    }

    fn encode_filtered(&mut self, octx: &mut format::context::Output) -> Result<(), MediaError> {
        let mut filtered = frame::Audio::empty();
        loop {
            let mut sink = self.graph.get("out").ok_or_else(|| failed("filter graph lost its output"))?;
            if sink.sink().frame(&mut filtered).is_err() {
                return Ok(());
            }
            let sink_tb = sink.sink().time_base();
            let pts = filtered.pts().map(|pts| pts.rescale(sink_tb, self.time_base));
            filtered.set_pts(pts);
            self.encoder.send_frame(&filtered)?;
            drain_encoder(&mut self.encoder, self.time_base, 0, self.out_idx, octx)?;
        }
    }
}

/// Passes a stream through untouched.
struct CopyPipe {
    out_idx: usize,
    time_base: Rational,
}

impl CopyPipe {
    fn new(
        istream: &format::stream::Stream,
        octx: &mut format::context::Output,
    ) -> Result<Self, MediaError> {
        let mut ostream = octx.add_stream(encoder::find(istream.parameters().id()))?;
        ostream.set_parameters(istream.parameters());
        // The source container's tag may mean nothing in the new one; let
        // the muxer choose.
        // SAFETY: the stream owns valid codec parameters for its lifetime.
        unsafe {
            (*ostream.parameters().as_mut_ptr()).codec_tag = 0;
        }
        Ok(Self { out_idx: ostream.index(), time_base: istream.time_base() })
    }

    fn push(
        &mut self,
        mut packet: Packet,
        octx: &mut format::context::Output,
    ) -> Result<(), MediaError> {
        let out_tb = octx.stream(self.out_idx).map_or(self.time_base, |s| s.time_base());
        packet.set_stream(self.out_idx);
        packet.rescale_ts(self.time_base, out_tb);
        packet.set_position(-1);
        packet.write_interleaved(octx)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_size_only_scales_down() {
        assert_eq!(Resolution::P720.output_size(1920, 1080), (1280, 720));
        assert_eq!(Resolution::P1080.output_size(1280, 720), (1280, 720));
        assert_eq!(Resolution::Source.output_size(1920, 1080), (1920, 1080));
    }

    #[test]
    fn output_size_is_always_even() {
        // 16:9 at 480 rows is 853.33 columns wide.
        assert_eq!(Resolution::P480.output_size(1280, 720), (854, 480));
        assert_eq!(Resolution::Source.output_size(853, 481), (852, 480));
        assert_eq!(Resolution::P720.output_size(1000, 1000), (720, 720));
    }

    #[test]
    fn settings_pick_the_encoder() {
        let mut settings = TranscodeSettings::default();
        assert_eq!(settings.encoder_name(), "libx264");
        settings.codec = VideoCodec::Hevc;
        settings.backend = EncoderBackend::Nvenc;
        assert_eq!(settings.encoder_name(), "hevc_nvenc");
        settings.container = Container::Mkv;
        assert_eq!(settings.extension(), "mkv");
    }

    #[test]
    fn switching_codec_keeps_the_picture_quality() {
        let h264 = TranscodeSettings { crf: 23, ..TranscodeSettings::default() };
        let hevc = h264.with_codec(VideoCodec::Hevc);
        assert_eq!((hevc.codec, hevc.crf), (VideoCodec::Hevc, 27));
        assert_eq!(hevc.with_codec(VideoCodec::Av1).crf, 33);
        assert_eq!(hevc.with_codec(VideoCodec::Vp9).crf, 31);
        assert_eq!(hevc.with_codec(VideoCodec::H264), h264);
        // Same codec: nothing moves.
        assert_eq!(hevc.with_codec(VideoCodec::Hevc), hevc);
        // Both ends of the slider map onto each other.
        for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9] {
            let best = TranscodeSettings { crf: 14, ..h264 }.with_codec(codec);
            let smallest = TranscodeSettings { crf: 36, ..h264 }.with_codec(codec);
            assert_eq!(best.crf..=smallest.crf, codec.crf_range());
        }
    }

    #[test]
    fn unsupported_gpu_codecs_fall_back_to_the_cpu() {
        let gpu = NvencSupport { h264: true, hevc: true, av1: false };
        let nvenc = TranscodeSettings { backend: EncoderBackend::Nvenc, ..TranscodeSettings::default() };
        assert_eq!(nvenc.limited_to(gpu).backend, EncoderBackend::Nvenc);
        let av1 = nvenc.with_codec(VideoCodec::Av1);
        assert_eq!(av1.backend, EncoderBackend::Nvenc);
        assert_eq!(av1.limited_to(gpu).backend, EncoderBackend::Software);
        assert_eq!(nvenc.limited_to(NvencSupport::default()).backend, EncoderBackend::Software);
        assert!(!NvencSupport { h264: true, hevc: true, av1: true }.supports(VideoCodec::Vp9));
    }

    #[test]
    fn quality_presets_follow_the_codec_scale() {
        assert_eq!(QualityPreset::High.crf(VideoCodec::H264), 18);
        assert_eq!(QualityPreset::Medium.crf(VideoCodec::H264), 23);
        assert_eq!(QualityPreset::Low.crf(VideoCodec::H264), 28);
        assert_eq!(QualityPreset::Medium.crf(VideoCodec::Av1), 33);
        // The default is the medium preset, and a preset survives a codec switch.
        let settings = TranscodeSettings::default();
        assert_eq!(settings.crf, QualityPreset::Medium.crf(settings.codec));
        for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1, VideoCodec::Vp9] {
            for preset in QualityPreset::ALL {
                assert!(codec.crf_range().contains(&preset.crf(codec)));
                let high = TranscodeSettings { crf: QualityPreset::High.crf(VideoCodec::H264), ..settings };
                assert_eq!(high.with_codec(codec).crf, QualityPreset::High.crf(codec));
            }
        }
    }

    #[test]
    fn vp9_falls_back_to_the_cpu_encoder() {
        let nvenc = TranscodeSettings {
            backend: EncoderBackend::Nvenc,
            ..TranscodeSettings::default()
        };
        assert_eq!(nvenc.with_codec(VideoCodec::Av1).encoder_name(), "av1_nvenc");
        let vp9 = nvenc.with_codec(VideoCodec::Vp9);
        assert_eq!(vp9.backend, EncoderBackend::Software);
        assert_eq!(vp9.encoder_name(), "libvpx-vp9");
        assert_eq!(vp9.encoder_options().get("crf"), Some("31"));
        // Constant quality on VP9 needs the bitrate zeroed.
        assert_eq!(vp9.encoder_options().get("b"), Some("0"));
    }

    #[test]
    fn rate_mode_picks_the_encoder_options() {
        let quality = TranscodeSettings { crf: 20, ..TranscodeSettings::default() };
        assert_eq!(quality.encoder_options().get("crf"), Some("20"));
        assert_eq!(quality.encoder_options().get("b"), None);

        let bitrate = TranscodeSettings {
            rate_mode: RateMode::Bitrate,
            bitrate_kbps: 4_000,
            ..quality
        };
        assert_eq!(bitrate.encoder_options().get("b"), Some("4000k"));
        assert_eq!(bitrate.encoder_options().get("maxrate"), Some("6000k"));
        assert_eq!(bitrate.encoder_options().get("crf"), None);

        let nvenc = TranscodeSettings { backend: EncoderBackend::Nvenc, ..quality };
        assert_eq!(nvenc.encoder_options().get("cq"), Some("20"));
        assert_eq!(nvenc.encoder_options().get("b"), Some("0"));
    }
}
