use windows::Win32::Foundation::{HMODULE, BOOL};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::core::{PCSTR, PSTR};

pub mod gw;
pub mod memory;


use hudhook::{hudhook, ImguiRenderLoop};
use hudhook::imgui;

struct ZoneTimer {

}


struct MyRenderLoop {

}

impl ImguiRenderLoop for MyRenderLoop {
    fn render(&mut self, ui: &mut imgui::Ui) {
        ui.window("My first render loop")
            .position([0., 0.], imgui::Condition::FirstUseEver)
            .size([320., 200.], imgui::Condition::FirstUseEver)
            .build(|| {
                ui.text("Hello, hello!");
            });
    }
}


use hudhook::hooks::dx9::ImguiDx9Hooks;
hudhook::hudhook!(ImguiDx9Hooks, MyRenderLoop{});