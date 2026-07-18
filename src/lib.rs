pub mod gw;
pub mod memory;
pub mod run_log;

use run_log::RunLog;


use std::time::{Duration, Instant};

use hudhook::ImguiRenderLoop;
use hudhook::RenderContext;
use hudhook::imgui::{
    self, Condition, FontConfig, FontId, FontSource, TableColumnSetup, TableFlags, WindowFlags,
};

use run_log::RunEntry;

/// Pixel size of the large timer font, loaded once in `initialize`.
const TIMER_FONT_PX: f32 = 48.0;

/// TrueType font used for the large timer readout, embedded at compile time so
/// there's no runtime file dependency inside the injected process.
static TIMER_FONT_TTF: &[u8] = include_bytes!("../res/default.ttf");

/// `FontId` holds a raw `*const Font`, which is neither `Send` nor `Sync`, but
/// hudhook requires the render loop to be both. The id is only ever created and
/// used on the render thread, so asserting these bounds is sound.
struct SyncFont(FontId);
unsafe impl Send for SyncFont {}
unsafe impl Sync for SyncFont {}

/// A stopwatch-style overlay timer.
///
/// It sits on top of the game as a borderless, click-through readout. Holding
/// the edit chord (Ctrl+Shift) reveals a bounding box and makes it draggable,
/// with a right-click context menu for start/pause/reset.
struct ZoneTimer {
    /// Time banked from previous run segments (before the current pause point).
    accumulated: Duration,
    /// If running, the instant the current segment started; `None` when paused.
    running_since: Option<Instant>,
    /// Top-left screen position of the timer window, in pixels.
    pos: [f32; 2],
    /// The large font, set up in `initialize`.
    big_font: Option<SyncFont>,
    /// Persistent log of completed runs, opened in `initialize`.
    run_log: Option<RunLog>,
    /// Runs logged during this game session, newest last.
    session_runs: Vec<RunEntry>,
    /// Whether the session-log window is open.
    show_log: bool,
}

impl Default for ZoneTimer {
    fn default() -> Self {
        Self {
            accumulated: Duration::ZERO,
            running_since: None,
            pos: [40.0, 40.0],
            big_font: None,
            run_log: None,
            session_runs: Vec::new(),
            show_log: false,
        }
    }
}

impl ZoneTimer {
    /// Total elapsed time, including the in-progress segment if running.
    fn elapsed(&self) -> Duration {
        self.accumulated + self.running_since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    fn start(&mut self) {
        if self.running_since.is_none() {
            self.running_since = Some(Instant::now());
        }
    }

    fn pause(&mut self) {
        if let Some(t) = self.running_since.take() {
            self.accumulated += t.elapsed();
        }
    }

    fn toggle(&mut self) {
        if self.is_running() {
            self.pause();
        } else {
            self.start();
        }
    }

    fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
        self.running_since = None;
    }

    /// Current time on the timer, formatted `MM:SS.d`.
    fn format_time(&self) -> String {
        format_duration(self.elapsed())
    }

    /// Log a completed run to the database and this session's list. Intended to
    /// be called by the (upcoming) map-change detection.
    #[allow(dead_code)]
    fn record_run(&mut self, from_map_id: u32, run_map_id: u32, to_map_id: u32, duration: Duration) {
        let Some(log) = &self.run_log else { return };
        match log.log_run(from_map_id, run_map_id, to_map_id, duration) {
            Ok(entry) => self.session_runs.push(entry),
            Err(e) => tracing::error!("failed to log run: {e}"),
        }
    }

    /// Draw the session-log window listing every run recorded this session.
    fn render_log_window(&mut self, ui: &imgui::Ui) {
        if !self.show_log {
            return;
        }

        // The edit chord unlocks dragging and the context menu.
        let edit = ui.io().key_ctrl && ui.io().key_shift;

        let mut flags =  WindowFlags::empty();
        if !edit {
            // Locked: can't be moved, and lets mouse input pass through to the game.
            flags |= WindowFlags::NO_MOVE | WindowFlags::NO_RESIZE | WindowFlags::NO_INPUTS | WindowFlags::NO_SCROLLBAR;
        }


        let mut open = true;
        let mut win =  ui.window("Session Run Log")
            .size([460.0, 300.0], Condition::FirstUseEver)
            .flags(flags);
            
            if edit {
                win = win.opened(&mut open);
            }


            win.build(|| {
                ui.text(format!("ctx ptr: {:X}", gw::get_context_tls().value()));
                if self.session_runs.is_empty() {
                    ui.text_disabled("No runs logged yet this session.");
                    return;
                }

                ui.text(format!("{} run(s) this session", self.session_runs.len()));
                ui.separator();

                let table = ui.begin_table_header_with_flags(
                    "session_runs",
                    [
                        TableColumnSetup::new("#"),
                        TableColumnSetup::new("From"),
                        TableColumnSetup::new("Run"),
                        TableColumnSetup::new("To"),
                        TableColumnSetup::new("Time"),
                    ],
                    TableFlags::ROW_BG | TableFlags::BORDERS | TableFlags::SIZING_STRETCH_PROP,
                );
                if let Some(_table) = table {
                    // Newest run first.
                    for (rank, run) in self.session_runs.iter().rev().enumerate() {
                        ui.table_next_row();
                        ui.table_next_column();
                        ui.text((self.session_runs.len() - rank).to_string());
                        ui.table_next_column();
                        ui.text(run.from_map_id.to_string());
                        ui.table_next_column();
                        ui.text(run.run_map_id.to_string());
                        ui.table_next_column();
                        ui.text(run.to_map_id.to_string());
                        ui.table_next_column();
                        ui.text(format_duration(run.duration));
                    }
                }
            });
        self.show_log = open;
    }
}

/// Format a duration as `MM:SS.d` with tenth-of-a-second precision.
fn format_duration(d: Duration) -> String {
    let hundos_total = d.as_millis() / 10;
    let hundos = hundos_total % 100;
    let secs_total = hundos_total / 100;
    let seconds = secs_total % 60;
    let minutes = secs_total / 60;
    format!("{minutes:02}:{seconds:02}.{hundos}")
}

impl ImguiRenderLoop for ZoneTimer {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        let fonts = ctx.fonts();
        // Add the normal-size default font first so it stays ImGui's global
        // default (font index 0) — everything except the timer uses this.
        fonts.add_font(&[FontSource::DefaultFontData { config: None }]);
        // The large font is a separate TTF entry we push only around the timer
        // text; rasterizing at the target size (vs. scaling the bitmap default)
        // is what gives smooth glyphs.
        let id = fonts.add_font(&[FontSource::TtfData {
            data: TIMER_FONT_TTF,
            size_pixels: TIMER_FONT_PX,
            config: Some(FontConfig {
                oversample_h: 3,
                ..FontConfig::default()
            }),
        }]);
        self.big_font = Some(SyncFont(id));

        match RunLog::open(RunLog::default_path()) {
            Ok(log) => self.run_log = Some(log),
            Err(e) => tracing::error!("failed to open run log: {e}"),
        }
    }

    fn render(&mut self, ui: &mut imgui::Ui) {
        // The edit chord unlocks dragging and the context menu.
        let edit = ui.io().key_ctrl && ui.io().key_shift;

        let mut flags = WindowFlags::NO_TITLE_BAR
            | WindowFlags::NO_RESIZE
            | WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::ALWAYS_AUTO_RESIZE;
            //| WindowFlags::NO_BACKGROUND;
        if !edit {
            // Locked: can't be moved, and lets mouse input pass through to the game.
            flags |= WindowFlags::NO_MOVE | WindowFlags::NO_INPUTS;
        }

        // While locked, hard-pin to `self.pos` every frame. While editing, only
        // seed position on first appearance so imgui's own drag can take over;
        // we read the dragged position back into `self.pos` below.
        let pos_cond = if edit { Condition::Appearing } else { Condition::Always };

        ui.window("##zone_timer")
            .position(self.pos, pos_cond)
            .flags(flags)
            .build(|| {
                let color = if self.is_running() {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [1.0, 0.75, 0.2, 1.0] // amber when paused
                };
                // Push the large font only for the time readout; the context
                // menu and everything else keep the default UI size.
                let font = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                ui.text_colored(color, self.format_time());
                if let Some(font) = font {
                    font.pop();
                }

                if edit {
                    // Capture the (possibly dragged) position so it survives the
                    // switch back to the locked, position-pinned state.
                    self.pos = ui.window_pos();

                    // Draw the bounding box that signals the timer is editable.
                    let p = ui.window_pos();
                    let s = ui.window_size();
                    ui.get_foreground_draw_list()
                        .add_rect(
                            [p[0] - 4.0, p[1] - 4.0],
                            [p[0] + s[0] + 4.0, p[1] + s[1] + 4.0],
                            [0.3, 0.8, 1.0, 0.9],
                        )
                        .thickness(2.0)
                        .rounding(3.0)
                        .build();

                    if let Some(_menu) = ui.begin_popup_context_window() {
                        let label = if self.is_running() { "Pause" } else { "Start" };
                        if ui.menu_item(label) {
                            self.toggle();
                        }
                        if ui.menu_item("Reset") {
                            self.reset();
                        }
                        ui.separator();
                        if ui
                            .menu_item_config("Session Log")
                            .selected(self.show_log)
                            .build()
                        {
                            self.show_log = !self.show_log;
                        }
                    }
                }
            });

        self.render_log_window(ui);
    }
}

use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, ZoneTimer::default());