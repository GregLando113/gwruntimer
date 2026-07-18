use windows::Win32::Foundation::{HMODULE, BOOL};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::core::{PCSTR, PSTR};

pub mod gw;
pub mod memory;

static mut G_HMODULE: HMODULE = HMODULE(std::ptr::null_mut());

use hudhook::*;

pub struct MyRenderLoop;

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


fn initialize() {
    use hudhook::hooks::dx9::ImguiDx9Hooks;
    hudhook!(ImguiDx9Hooks, MyRenderLoop);
}

fn deinitialize() {
}




#[unsafe(no_mangle)]
pub extern "system" fn DllMain(hmodule: HMODULE, dw_reason: u32, _lp_reserved: *const core::ffi::c_void) -> BOOL {
    match dw_reason {
        DLL_PROCESS_ATTACH => {
            unsafe { G_HMODULE = hmodule; };
            unsafe { let _ = DisableThreadLibraryCalls(hmodule); }
            std::thread::spawn(|| {
                initialize()
            });
            BOOL::from(true)
        }
        DLL_PROCESS_DETACH => {
            deinitialize();
            BOOL::from(true)
        }
        _ => BOOL::from(true)
    }
}