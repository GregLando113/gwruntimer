mod gw;
mod memory;
mod run_log;


use run_log::RunLog;


use std::time::{Duration, Instant};

use hudhook::ImguiRenderLoop;
use hudhook::RenderContext;
use hudhook::imgui::{
    self, Condition, FontConfig, FontId, FontSource, StyleColor, TableColumnFlags, TableColumnSetup,
    TableFlags, TreeNodeFlags, WindowFlags,
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


struct ZoneTimerState {
    last_load_state: bool,
    mapid_before_run: u32,
    mapid_during_run: u32,
    mapid_now: u32
}

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
    /// Edit-mode state from the previous frame, for detecting enter/leave.
    was_edit: bool,

    state: ZoneTimerState,
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
            was_edit: false,
            state: ZoneTimerState{
                last_load_state: false,
                mapid_before_run: 0,
                mapid_during_run: 0,
                mapid_now: 0
            }
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

    /// Draw the table of runs recorded this session. Meant to be called inside
    /// the main window, under the session-log collapsing header.
    fn render_session_table(&self, ui: &imgui::Ui) {
        if self.session_runs.is_empty() {
            ui.text_disabled("No runs logged yet this session.");
            return;
        }

        // The time is always "MM:SS.hh", so pin that column to a constant width
        // (its text width plus a small pad) rather than letting it size to data.
        let time_width = ui.calc_text_size("00:00.00")[0] + 5.0;
        let table = ui.begin_table_header_with_flags(
            "session_runs",
            [
                // Under SIZING_FIXED_FIT these three size to their content.
                TableColumnSetup{
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 175.0,
                    ..TableColumnSetup::new("From")
                },
                
                // Under SIZING_FIXED_FIT these three size to their content.
                TableColumnSetup{
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 175.0,
                    ..TableColumnSetup::new("Through")
                },
                TableColumnSetup{
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: 175.0,
                    ..TableColumnSetup::new("To")
                },
                TableColumnSetup {
                    flags: TableColumnFlags::WIDTH_FIXED,
                    init_width_or_weight: time_width,
                    ..TableColumnSetup::new("Time")
                },
            ],
            // Fixed-fit columns give the table a definite width, which the
            // auto-resizing window needs (stretch columns have nothing to
            // stretch into).
            // TableFlags::ROW_BG | TableFlags::BORDERS |
            TableFlags::SIZING_FIXED_FIT,
        );
        if let Some(_table) = table {
            // Newest run first.
            for run in self.session_runs.iter().rev() {
                ui.table_next_row();
                ui.table_next_column();
                ui.text(map_name_or_id(run.from_map_id));
                ui.table_next_column();
                ui.text(map_name_or_id(run.run_map_id));
                ui.table_next_column();
                ui.text(map_name_or_id(run.to_map_id));
                ui.table_next_column();
                ui.text(format_duration(run.duration));
            }
        }
    }
}

/// Display string for a map: its decoded name if available, otherwise the
/// numeric id as a fallback while the async decode is still in flight.
fn map_name_or_id(mapid: u32) -> String {
    gw::mapdata::map_name(mapid).unwrap_or_else(|| mapid.to_string())
}

/// Format a duration as `MM:SS.d` with tenth-of-a-second precision.
fn format_duration(d: Duration) -> String {
    let hundos_total = d.as_millis() / 10;
    let hundos = hundos_total % 100;
    let secs_total = hundos_total / 100;
    let seconds = secs_total % 60;
    let minutes = secs_total / 60;
    format!("{minutes:02}:{seconds:02}.{hundos:02}")
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

        match RunLog::in_memory() {
            Ok(log) => self.run_log = Some(log),
            Err(e) => tracing::error!("failed to open run log: {e}"),
        }
    }

    fn render(&mut self, ui: &mut imgui::Ui) {


        // timer logic

        let map_data_ptr = gw::MissionData::current();
        let movement_ptr  = gw::CharControlledData::current();

        if map_data_ptr.is_loaded() {
            if self.state.last_load_state == false {
                let cur_map_id = map_data_ptr.map_id();
                self.state.mapid_before_run = self.state.mapid_during_run;
                self.state.mapid_during_run = self.state.mapid_now;
                self.state.mapid_now = cur_map_id;
                if !self.elapsed().is_zero() {
                    self.record_run(
                        self.state.mapid_before_run, 
                        self.state.mapid_during_run,
                        self.state.mapid_now, 
                    self.elapsed());
                    self.reset();
                }
            }
            if map_data_ptr.is_explorable() {
                if !self.is_running() && movement_ptr.is_moving() {
                    self.start();
                }
            }
        }
        else {
            if self.is_running() {
                self.pause();
            }
        }
        self.state.last_load_state = map_data_ptr.is_loaded();



        // The edit chord unlocks dragging and the context menu.
        let edit = ui.io().key_ctrl && ui.io().key_shift;
        // Detect the frame edit mode turns on/off, to grab and release focus.
        let entered_edit = edit && !self.was_edit;
        let left_edit = !edit && self.was_edit;
        self.was_edit = edit;

        let mut flags = WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::ALWAYS_AUTO_RESIZE;
        if !edit {
            // Locked: can't be moved, and lets mouse input pass through to the game.
            flags |= WindowFlags::NO_RESIZE | WindowFlags::NO_MOVE | WindowFlags::NO_INPUTS;

            if !self.show_log {
                flags |= WindowFlags::NO_TITLE_BAR;
            }
        }

        // While locked, hard-pin to `self.pos` every frame. While editing, only
        // seed position on first appearance so imgui's own drag can take over;
        // we read the dragged position back into `self.pos` below.
        let pos_cond = if edit { Condition::Appearing } else { Condition::Always };

        let mut win = ui
            .window("KAOS' Zone Timer##zone_timer")
            .position(self.pos, pos_cond)
            // Bring the window to the front on the frame we enter edit mode;
            // `.focused(false)` is a no-op, so this only fires on the transition.
            .focused(entered_edit)
            .flags(flags);
        // When the log table is showing, hold the window to at least 300x700 so
        // the run list has room; otherwise it auto-resizes to hug the timer.
        if self.show_log {
            win = win.size_constraints([600.0, 400.0], [f32::MAX, f32::MAX]);
        }

        win.build(|| {
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
                }

                // Session-log panel. The collapsing header is only interactive
                // in edit mode (the window is click-through when locked). When
                // locked and collapsed, neither the header nor the table shows.
                if edit || self.show_log {
                    ui.separator();
                    // `show_log` is the source of truth: force the header's open
                    // state from it each frame. imgui's own collapse state lives
                    // in per-context storage that's wiped when the D3D9 device
                    // resets (e.g. on a map change), which would otherwise make
                    // the panel spuriously collapse. A user click still toggles:
                    // it flips the forced value, and we read the result back.
                    unsafe {
                        imgui::sys::igSetNextItemOpen(self.show_log, Condition::Always as i32);
                    }
                    // When locked, paint the header bar the window background
                    // color so it blends in instead of showing the accent color.
                    let header_colors = (!edit).then(|| {
                        let bg = ui.style_color(StyleColor::WindowBg);
                        [
                            ui.push_style_color(StyleColor::Header, bg),
                            ui.push_style_color(StyleColor::HeaderHovered, bg),
                            ui.push_style_color(StyleColor::HeaderActive, bg),
                        ]
                    });
                    self.show_log = ui.collapsing_header("Session Log", TreeNodeFlags::empty());
                    drop(header_colors);

                    if self.show_log {
                        self.render_session_table(ui);
                    }
                }

                if edit {
                    if let Some(_menu) = ui.begin_popup_context_window() {
                        
                         if ui
                            .menu_item_config("Clear Log")
                            .build()
                        {
                            self.session_runs.clear();
                        }

                        if ui
                            .menu_item_config("Exit Timer (WIP)")
                            .build()
                        {
                            // to be implemented
                        }
                    }
                }
            });

        if left_edit {
            // Release focus back to the game when leaving edit mode.
            unsafe { imgui::sys::igSetWindowFocus_Nil() };
        }
    }
}

use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, ZoneTimer::default());