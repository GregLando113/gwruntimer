//! GW Timer launcher: pick a running Guild Wars client and inject the timer DLL.
#![windows_subsystem = "windows"]

mod clients;
mod inject;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use native_windows_derive as nwd;
use native_windows_gui as nwg;

use nwd::NwgUi;
use nwg::NativeUi;

/// DLL filename produced by the `gw-run-timer` crate ([lib] name = kaos_zone_timer).
const DLL_NAME: &str = "kaos_zone_timer.dll";

#[derive(Default, NwgUi)]
pub struct LauncherApp {
    /// Enumerated clients, index-aligned with the list box entries.
    clients: RefCell<Vec<clients::GwClient>>,

    #[nwg_control(size: (380, 320), position: (400, 300), title: "GW Timer Launcher", flags: "WINDOW|VISIBLE")]
    #[nwg_events( OnWindowClose: [nwg::stop_thread_dispatch()], OnInit: [LauncherApp::on_init] )]
    window: nwg::Window,

    #[nwg_layout(parent: window, spacing: 4, margin: [8, 8, 8, 8])]
    grid: nwg::GridLayout,

    #[nwg_control(collection: Vec::new())]
    #[nwg_layout_item(layout: grid, col: 0, row: 0, col_span: 2, row_span: 5)]
    list: nwg::ListBox<String>,

    #[nwg_control(text: "Scanning for clients…")]
    #[nwg_layout_item(layout: grid, col: 0, row: 5, col_span: 2)]
    status: nwg::Label,

    #[nwg_control(text: "Refresh")]
    #[nwg_layout_item(layout: grid, col: 0, row: 6)]
    #[nwg_events( OnButtonClick: [LauncherApp::on_refresh] )]
    refresh_btn: nwg::Button,

    #[nwg_control(text: "Inject")]
    #[nwg_layout_item(layout: grid, col: 1, row: 6)]
    #[nwg_events( OnButtonClick: [LauncherApp::on_inject] )]
    inject_btn: nwg::Button,
}

impl LauncherApp {
    fn on_init(&self) {
        self.refresh();
    }

    fn on_refresh(&self) {
        self.refresh();
    }

    /// Re-enumerate clients and repopulate the list box.
    fn refresh(&self) {
        match clients::enumerate() {
            Ok(found) => {
                let labels: Vec<String> = found.iter().map(|c| c.label()).collect();
                let count = found.len();
                *self.clients.borrow_mut() = found;
                self.list.set_collection(labels);
                let msg = if count == 0 {
                    format!("No {} clients found. Launch GW, then Refresh.", clients_exe())
                } else {
                    format!("{count} client(s) found. Select one and click Inject.")
                };
                self.status.set_text(&msg);
            }
            Err(e) => self.status.set_text(&format!("Enumeration failed: {e}")),
        }
    }

    fn on_inject(&self) {
        let idx = match self.list.selection() {
            Some(i) => i,
            None => {
                self.status.set_text("Select a client first.");
                return;
            }
        };

        // Clone out before injecting so we don't hold the borrow across the call.
        let client = match self.clients.borrow().get(idx) {
            Some(c) => c.clone(),
            None => {
                self.status.set_text("Selection is stale — Refresh and try again.");
                return;
            }
        };

        let dll = match dll_path() {
            Ok(p) => p,
            Err(e) => {
                nwg::modal_error_message(&self.window, "DLL not found", &e);
                self.status.set_text("Cannot inject: DLL missing.");
                return;
            }
        };

        match inject::inject_pid(client.pid, &dll) {
            Ok(()) => self
                .status
                .set_text(&format!("Injected into pid {}.", client.pid)),
            Err(e) => {
                nwg::modal_error_message(&self.window, "Injection failed", &e);
                self.status.set_text("Injection failed.");
            }
        }
    }
}

/// Exe name shown to the user (kept in sync with `clients::GW_EXE`).
fn clients_exe() -> &'static str {
    "Gw.exe"
}

/// Resolves the timer DLL, expected to sit next to the launcher executable.
fn dll_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe() failed: {e}"))?;
    let dir: &Path = exe
        .parent()
        .ok_or("launcher executable has no parent directory")?;
    let dll = dir.join(DLL_NAME);
    if !dll.exists() {
        return Err(format!(
            "{DLL_NAME} not found next to the launcher:\n{}",
            dll.display()
        ));
    }
    Ok(dll)
}

fn main() {
    nwg::init().expect("Failed to init Native Windows GUI");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let _app = LauncherApp::build_ui(Default::default()).expect("Failed to build UI");
    nwg::dispatch_thread_events();
}
