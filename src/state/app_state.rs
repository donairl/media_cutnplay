use crate::core::cutter::{insert_segment, CutMode, Segment};
use crate::core::metadata::MediaMetadata;
use crate::core::transcode::{NvencSupport, TranscodeSettings};
use crate::worker::commands::WorkerCommand;
use crate::player::{Player, VideoFrame};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Processing { progress: f32, output_path: PathBuf },
    /// The files written, in segment order.
    Completed { outputs: Vec<PathBuf> },
    Failed(String),
}

/// Picture adjustments applied to the preview on the GPU. They never touch
/// the exported file: cuts are lossless stream copies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoAdjust {
    /// -1.0 (darkest) ..= 1.0 (brightest); 0.0 leaves the picture unchanged.
    pub brightness: f32,
    /// -1.0 (flat grey) ..= 1.0 (double contrast); 0.0 leaves it unchanged.
    pub contrast: f32,
    /// -1.0 (no colour) ..= 1.0 (double saturation); 0.0 leaves it unchanged.
    pub saturation: f32,
    pub monochrome: bool,
}

impl Default for VideoAdjust {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            monochrome: false,
        }
    }
}

impl VideoAdjust {
    pub fn get(&self, kind: AdjustKind) -> f32 {
        match kind {
            AdjustKind::Brightness => self.brightness,
            AdjustKind::Contrast => self.contrast,
            AdjustKind::Saturation => self.saturation,
        }
    }

    pub fn set(&mut self, kind: AdjustKind, value: f32) {
        let value = value.clamp(-1.0, 1.0);
        match kind {
            AdjustKind::Brightness => self.brightness = value,
            AdjustKind::Contrast => self.contrast = value,
            AdjustKind::Saturation => self.saturation = value,
        }
    }
}

/// The slider-driven adjustments, each with its own action-bar flyout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustKind {
    Brightness,
    Contrast,
    Saturation,
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
    /// Shares the right-hand side with the export panel: one at a time.
    pub show_transcode_panel: bool,
    /// Preview picture settings and the flyout currently open for them.
    pub adjust: VideoAdjust,
    pub adjust_panel: Option<AdjustKind>,
    pub current_timestamp: f64,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    /// Committed cut segments of the active file: sorted, non-overlapping.
    pub segments: Vec<Segment>,
    /// Join the segments into one output file instead of one file each.
    pub merge_segments: bool,
    pub cut_mode: CutMode,
    pub current_frame: Option<Arc<VideoFrame>>,

    pub target_output_path: Option<PathBuf>,
    pub export_status: ExportStatus,

    pub transcode: TranscodeSettings,
    pub transcode_output_path: Option<PathBuf>,
    pub transcode_status: ExportStatus,
    /// Raised to stop the running transcode (and the rest of its run).
    pub transcode_cancel: Arc<AtomicBool>,
    /// (input, output) jobs of the current run still waiting to start.
    pub transcode_pending: VecDeque<(PathBuf, PathBuf)>,
    /// Number of jobs in the current run (1 for a single file).
    pub transcode_total: usize,
    pub transcode_outputs: Vec<PathBuf>,
    pub transcode_errors: Vec<String>,
    /// What the GPU can encode; `None` until the startup probe reports.
    pub nvenc: Option<NvencSupport>,

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
            show_transcode_panel: false,
            adjust: VideoAdjust::default(),
            adjust_panel: None,
            current_timestamp: 0.0,
            in_point: None,
            out_point: None,
            segments: Vec::new(),
            merge_segments: false,
            cut_mode: CutMode::default(),
            current_frame: None,
            target_output_path: None,
            export_status: ExportStatus::Idle,
            transcode: TranscodeSettings::default(),
            transcode_output_path: None,
            transcode_status: ExportStatus::Idle,
            transcode_cancel: Arc::new(AtomicBool::new(false)),
            transcode_pending: VecDeque::new(),
            transcode_total: 0,
            transcode_outputs: Vec::new(),
            transcode_errors: Vec::new(),
            nvenc: None,
            ram_usage_mb: ram_mb,
            detected_hw: "Decode: idle".to_string(),
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

    /// The in/out pair being marked, when it spans a positive duration.
    pub fn pending_segment(&self) -> Option<Segment> {
        match (self.in_point, self.out_point) {
            (Some(start), Some(end)) if end > start => Some(Segment { start, end }),
            _ => None,
        }
    }

    /// Moves the pending in/out pair into the segment list.
    pub fn commit_pending_segment(&mut self) {
        if let Some(segment) = self.pending_segment() {
            insert_segment(&mut self.segments, segment);
            self.in_point = None;
            self.out_point = None;
        }
    }

    /// What an export would cut: the segment list, or just the pending
    /// in/out pair while the list is empty.
    pub fn export_segments(&self) -> Vec<Segment> {
        if self.segments.is_empty() {
            self.pending_segment().into_iter().collect()
        } else {
            self.segments.clone()
        }
    }

    pub fn active_duration(&self) -> f64 {
        self.active_item().map(|m| m.duration_secs).unwrap_or(0.0)
    }

    /// 1-based position of the running job within its run.
    pub fn transcode_job_number(&self) -> usize {
        self.transcode_total - self.transcode_pending.len()
    }

    /// Starts a run of transcodes, one after another, with the current
    /// settings. Ignored while another run is busy.
    pub fn start_transcodes(&mut self, jobs: Vec<(PathBuf, PathBuf)>) {
        let busy = matches!(self.transcode_status, ExportStatus::Processing { .. });
        if busy || jobs.is_empty() {
            return;
        }
        self.transcode_total = jobs.len();
        self.transcode_pending = jobs.into();
        self.transcode_outputs.clear();
        self.transcode_errors.clear();
        self.start_next_transcode();
    }

    /// Records the running job's result (`None`: cancelled, which ends the
    /// whole run) and moves on to the next one.
    pub fn finish_transcode(&mut self, result: Option<Result<PathBuf, String>>) {
        let ExportStatus::Processing { output_path, .. } = &self.transcode_status else {
            return;
        };
        match result {
            Some(Ok(path)) => self.transcode_outputs.push(path),
            Some(Err(err)) => {
                let name = output_path.file_name().unwrap_or_default().to_string_lossy();
                self.transcode_errors.push(if self.transcode_total > 1 {
                    format!("{name}: {err}")
                } else {
                    err
                });
            }
            None => {
                self.transcode_pending.clear();
                self.transcode_status = ExportStatus::Idle;
                return;
            }
        }
        self.start_next_transcode();
    }

    /// Hands the next job to the worker, or settles the run's final status.
    fn start_next_transcode(&mut self) {
        let Some((input_path, output_path)) = self.transcode_pending.pop_front() else {
            self.transcode_status = match self.transcode_errors.len() {
                0 => ExportStatus::Completed { outputs: std::mem::take(&mut self.transcode_outputs) },
                failed if self.transcode_total > 1 => ExportStatus::Failed(format!(
                    "{failed} of {} failed. {}",
                    self.transcode_total,
                    self.transcode_errors.join("; ")
                )),
                _ => ExportStatus::Failed(self.transcode_errors.join("; ")),
            };
            return;
        };
        if output_path == input_path {
            self.transcode_errors.push("The destination is the source file itself".into());
            return self.start_next_transcode();
        }

        // A fresh flag per job: a late cancel cannot hit the next one.
        self.transcode_cancel = Arc::new(AtomicBool::new(false));
        self.transcode_status = ExportStatus::Processing { progress: 0.0, output_path: output_path.clone() };
        let _ = self.cmd_tx.try_send(WorkerCommand::StartTranscode {
            input_path,
            output_path,
            settings: self.transcode,
            cancel: self.transcode_cancel.clone(),
        });
    }
}

/// Where a transcode writes when no destination was chosen:
/// `<input>_transcoded.<ext>` next to the input.
pub fn default_transcode_output(input: &Path, extension: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("media");
    input.with_file_name(format!("{stem}_transcoded.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::AudioBackend;
    use tokio::sync::mpsc::{channel, Receiver};

    fn state() -> (AppState, Receiver<WorkerCommand>) {
        let (tx, rx) = channel(16);
        (AppState::new(tx, Player::spawn(AudioBackend::Disabled, |_| {})), rx)
    }

    fn started(rx: &mut Receiver<WorkerCommand>) -> Option<PathBuf> {
        match rx.try_recv().ok()? {
            WorkerCommand::StartTranscode { input_path, .. } => Some(input_path),
            _ => None,
        }
    }

    fn job(name: &str) -> (PathBuf, PathBuf) {
        (name.into(), default_transcode_output(Path::new(name), "mp4"))
    }

    #[test]
    fn run_goes_through_every_job_and_reports_failures() {
        let (mut s, mut rx) = state();
        let same = PathBuf::from("b.mp4");
        s.start_transcodes(vec![job("a.mkv"), (same.clone(), same), job("c.mkv")]);
        assert_eq!(started(&mut rx), Some("a.mkv".into()));
        assert_eq!(s.transcode_job_number(), 1);

        // A second run cannot start while this one is busy.
        s.start_transcodes(vec![job("x.mkv")]);
        assert_eq!(started(&mut rx), None);

        // b would overwrite its own source: skipped with an error, c runs.
        s.finish_transcode(Some(Ok("a_transcoded.mp4".into())));
        assert_eq!(started(&mut rx), Some("c.mkv".into()));
        assert_eq!(s.transcode_job_number(), 3);

        s.finish_transcode(Some(Err("boom".into())));
        assert_eq!(started(&mut rx), None);
        let ExportStatus::Failed(err) = &s.transcode_status else { panic!("{:?}", s.transcode_status) };
        assert!(err.starts_with("2 of 3 failed.") && err.contains("c_transcoded.mp4: boom"), "{err}");
    }

    #[test]
    fn cancel_stops_the_whole_run() {
        let (mut s, mut rx) = state();
        s.start_transcodes(vec![job("a.mkv"), job("b.mkv")]);
        assert!(started(&mut rx).is_some());
        s.finish_transcode(None);
        assert_eq!(started(&mut rx), None);
        assert_eq!(s.transcode_status, ExportStatus::Idle);
        assert!(s.transcode_pending.is_empty());
    }

    #[test]
    fn single_job_completes_with_its_output() {
        let (mut s, mut rx) = state();
        s.start_transcodes(vec![job("/v/a.mkv")]);
        assert!(started(&mut rx).is_some());
        s.finish_transcode(Some(Ok("/v/a_transcoded.mp4".into())));
        assert_eq!(
            s.transcode_status,
            ExportStatus::Completed { outputs: vec!["/v/a_transcoded.mp4".into()] }
        );
    }
}
