//! Hardware video decoding (NVDEC through CUDA, VAAPI).
//!
//! libavcodec does the heavy lifting: we hand the decoder a hardware device
//! and steer its pixel-format negotiation toward that device's surface
//! format. Decoded surfaces stay in video memory until [`download`] copies
//! them back. If the hardware turns a stream down during negotiation,
//! libavcodec quietly decodes it in software on the same context.

use ffmpeg_next::codec::context::Context;
use ffmpeg_next::ffi;
use ffmpeg_next::util::frame::video::Video as RawVideoFrame;
use ffmpeg_next::Codec;
use std::ffi::c_void;
use std::ptr;

/// Set to `cuda`, `vaapi` or `off` to override the automatic choice.
const OVERRIDE_VAR: &str = "CUTNPLAY_HWDEC";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwKind {
    Cuda,
    Vaapi,
}

impl HwKind {
    pub fn label(self) -> &'static str {
        match self {
            HwKind::Cuda => "NVDEC (CUDA)",
            HwKind::Vaapi => "VAAPI",
        }
    }

    fn device_type(self) -> ffi::AVHWDeviceType {
        match self {
            HwKind::Cuda => ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
            HwKind::Vaapi => ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        }
    }
}

/// Backends to try, best first. Software decoding is the caller's last resort.
pub fn candidates() -> Vec<HwKind> {
    match std::env::var(OVERRIDE_VAR).ok().as_deref() {
        Some("off" | "none" | "software" | "0") => Vec::new(),
        Some("cuda" | "nvdec") => vec![HwKind::Cuda],
        Some("vaapi") => vec![HwKind::Vaapi],
        _ => vec![HwKind::Cuda, HwKind::Vaapi],
    }
}

/// Prepares `ctx` to decode on `kind`. Returns the decoder to open it with,
/// or `None` when the codec has no such hardware path or the device cannot
/// be created (no such GPU, missing driver, ...).
pub fn attach(ctx: &mut Context, kind: HwKind) -> Option<Codec> {
    // Wrapper libraries (libdav1d, ...) are software-only; the hardware hooks
    // live in FFmpeg's native decoder, which is named after the codec.
    let id = ctx.id();
    let codec = ffmpeg_next::decoder::find_by_name(id.name())
        .filter(|codec| codec.id() == id)
        .or_else(|| ffmpeg_next::decoder::find(id))?;

    // SAFETY: `codec` and `ctx` are valid for the whole block. The device
    // reference created here is handed to the codec context, which releases
    // it when it is freed.
    unsafe {
        let surface_format = surface_format(codec.as_ptr(), kind.device_type())?;

        let mut device = ptr::null_mut();
        let created = ffi::av_hwdevice_ctx_create(
            &mut device,
            kind.device_type(),
            ptr::null(),
            ptr::null_mut(),
            0,
        );
        if created < 0 || device.is_null() {
            return None;
        }

        let raw = ctx.as_mut_ptr();
        (*raw).hw_device_ctx = device;
        // `pick_format` reads the wanted surface format back from `opaque`.
        (*raw).opaque = surface_format as i32 as usize as *mut c_void;
        (*raw).get_format = Some(pick_format);
    }

    Some(codec)
}

/// Whether `frame` is a surface in video memory rather than pixels in RAM.
pub fn is_hw_frame(frame: &RawVideoFrame) -> bool {
    // SAFETY: the frame owns a valid `AVFrame`.
    unsafe { !(*frame.as_ptr()).hw_frames_ctx.is_null() }
}

/// Copies a hardware surface into system memory (NV12, P010, ...).
pub fn download(frame: &RawVideoFrame) -> Option<RawVideoFrame> {
    let mut out = RawVideoFrame::empty();
    // SAFETY: both frames are valid; `out` is blank, so libavutil allocates
    // its buffers in the first format the device can download to.
    let status = unsafe { ffi::av_hwframe_transfer_data(out.as_mut_ptr(), frame.as_ptr(), 0) };
    (status >= 0).then_some(out)
}

/// The surface format `codec` produces on a device of `device_type`, if it
/// can decode through a plain device context at all.
unsafe fn surface_format(
    codec: *const ffi::AVCodec,
    device_type: ffi::AVHWDeviceType,
) -> Option<ffi::AVPixelFormat> {
    for index in 0.. {
        let config = unsafe { ffi::avcodec_get_hw_config(codec, index) };
        if config.is_null() {
            break;
        }
        let config = unsafe { &*config };
        let via_device =
            config.methods as i64 & ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i64 != 0;
        if via_device && config.device_type == device_type {
            return Some(config.pix_fmt);
        }
    }
    None
}

/// `get_format` callback: take the hardware surface format when the decoder
/// offers it, otherwise let libavcodec choose (which means software).
unsafe extern "C" fn pick_format(
    ctx: *mut ffi::AVCodecContext,
    formats: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    unsafe {
        let wanted = (*ctx).opaque as usize as i32;
        // Compared as raw integers: the list may hold formats this binding
        // has no enum variant for.
        let raw = formats.cast::<i32>();
        let mut index = 0;
        while *raw.add(index) != ffi::AVPixelFormat::AV_PIX_FMT_NONE as i32 {
            if *raw.add(index) == wanted {
                return *formats.add(index);
            }
            index += 1;
        }
        ffi::avcodec_default_get_format(ctx, formats)
    }
}
