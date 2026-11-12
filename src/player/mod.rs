//! Media playback engine.
//!
//! A dedicated thread demuxes and decodes audio + video. Audio goes to the
//! sound card through [`timeline::Timeline`], whose device-driven clock is the
//! master clock. Decoded video frames wait in a small queue and the UI pulls
//! whichever frame is due on every display refresh.

pub mod audio;
pub mod engine;
pub mod hwaccel;
pub mod timeline;

pub use audio::AudioBackend;
pub use engine::{Player, PlayerEvent, VideoFrame};
