use crate::core::metadata::MediaMetadata;
use crate::worker::commands::WorkerCommand;
use crate::player::{Player, VideoFrame};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Processing { progress: f32, output_path: PathBuf },
    Completed { output_path: PathBuf },
    Failed(String),
}

pub struct AppState {
    pub cmd_tx: Sender<WorkerCommand>,
    pub player: Player,
    pub queue: Vec<MediaMetadata>,
    pub active_index: Option<usize>,

    pub is_playing: bool,
    pub is_fullscreen: bool,
    /// Fullscreen controls: wanted state and slide progress (0 hidden, 1 shown).
    pub controls_visible: bool,
    pub controls_reveal: f32,
    pub controls_anim_last: Option<Instant>,
    pub cursor_y: Option<f32>,
    pub pointer_pressed: bool,
    pub window_height: f32,
    /// Side panels, toggled from the action bar.
    pub show_queue_panel: bool,
    pub show_export_panel: bool,
    pub current_timestamp: f64,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub current_frame: Option<Arc<VideoFrame>>,

    pub target_output_path: Option<PathBuf>,
    pub export_status: ExportStatus,

    pub ram_usage_mb: f32,
    pub detected_hw: String,
    pub system_monitor: System,
    pub current_pid: Pid,
}

impl AppState {
    pub fn new(cmd_tx: Sender<WorkerCommand>, player: Player) -> Self {
        let mut sys = System::new();
        let pid = Pid::from_u32(std::process::id());
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        let ram_mb = sys.process(pid).map(|p| p.memory() as f32 / (1024.0 * 1024.0)).unwrap_or(0.0);

        Self {
            cmd_tx,
            player,
            queue: Vec::new(),
            active_index: None,
            is_playing: false,
            is_fullscreen: false,
            controls_visible: false,
            controls_reveal: 0.0,
            controls_anim_last: None,
            cursor_y: None,
            pointer_pressed: false,
            window_height: 720.0,
            show_queue_panel: true,
            show_export_panel: true,
            current_timestamp: 0.0,
            in_point: None,
            out_point: None,
            current_frame: None,
            target_output_path: None,
            export_status: ExportStatus::Idle,
            ram_usage_mb: ram_mb,
            detected_hw: "Native libavcodec 63".to_string(),
            system_monitor: sys,
            current_pid: pid,
        }
    }

    pub fn refresh_memory(&mut self) {
        self.system_monitor.refresh_processes(ProcessesToUpdate::Some(&[self.current_pid]), true);
        if let Some(proc) = self.system_monitor.process(self.current_pid) {
            self.ram_usage_mb = proc.memory() as f32 / (1024.0 * 1024.0);
        }
    }

    /// True while the fullscreen controls are still sliding in or out.
    pub fn controls_animating(&self) -> bool {
        let target = if self.controls_visible { 1.0 } else { 0.0 };
        self.is_fullscreen && self.controls_reveal != target
    }

    pub fn active_item(&self) -> Option<&MediaMetadata> {
        self.active_index.and_then(|idx| self.queue.get(idx))
    }

    pub fn active_duration(&self) -> f64 {
        self.active_item().map(|m| m.duration_secs).unwrap_or(0.0)
    }
}
