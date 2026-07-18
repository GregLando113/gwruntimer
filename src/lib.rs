pub mod gw;
pub mod memory;


use std::time::{Duration, Instant};

use hudhook::ImguiRenderLoop;
use hudhook::RenderContext;
use hudhook::imgui::{self, Condition, FontConfig, FontId, FontSource, WindowFlags};

/// Pixel size of the large timer font, loaded once in `initialize`.
const TIMER_FONT_PX: f32 = 48.0;

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
}

impl Default for ZoneTimer {
    fn default() -> Self {
        Self {
            accumulated: Duration::ZERO,
            running_since: None,
            pos: [40.0, 40.0],
            big_font: None,
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

    /// Format as `MM:SS.d` with tenth-of-a-second precision.
    fn format_time(&self) -> String {
        let tenths_total = self.elapsed().as_millis() / 100;
        let tenths = tenths_total % 10;
        let secs_total = tenths_total / 10;
        let seconds = secs_total % 60;
        let minutes = secs_total / 60;
        format!("{minutes:02}:{seconds:02}.{tenths}")
    }
}

impl ImguiRenderLoop for ZoneTimer {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        let id = ctx.fonts().add_font(&[FontSource::DefaultFontData {
            config: Some(FontConfig {
                size_pixels: TIMER_FONT_PX,
                ..FontConfig::default()
            }),
        }]);
        self.big_font = Some(SyncFont(id));
    }

    fn render(&mut self, ui: &mut imgui::Ui) {
        // The edit chord unlocks dragging and the context menu.
        let edit = ui.io().key_ctrl && ui.io().key_shift;

        let mut flags = WindowFlags::NO_TITLE_BAR
            | WindowFlags::NO_RESIZE
            | WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::ALWAYS_AUTO_RESIZE
            | WindowFlags::NO_BACKGROUND;
        if !edit {
            // Locked: can't be moved, and lets mouse input pass through to the game.
            flags |= WindowFlags::NO_MOVE | WindowFlags::NO_INPUTS;
        }

        // While locked, hard-pin to `self.pos` every frame. While editing, only
        // seed position on first appearance so imgui's own drag can take over;
        // we read the dragged position back into `self.pos` below.
        let pos_cond = if edit { Condition::Appearing } else { Condition::Always };

        let font = self.big_font.as_ref().map(|f| ui.push_font(f.0));

        ui.window("##zone_timer")
            .position(self.pos, pos_cond)
            .flags(flags)
            .build(|| {
                let color = if self.is_running() {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [1.0, 0.75, 0.2, 1.0] // amber when paused
                };
                ui.text_colored(color, self.format_time());

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
                    }
                }
            });

        if let Some(font) = font {
            font.pop();
        }
    }
}

use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, ZoneTimer::default());