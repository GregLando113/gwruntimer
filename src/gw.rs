
use crate::memory::{Address,ProcessModule};
use std::sync::LazyLock;

static GET_BUILD_NUMBER_FN: LazyLock<extern "cdecl" fn() -> u32> = LazyLock::new( || {
    let result_fn = ProcessModule::main()
        .expect("Cant get main module.")
        .find_pattern("57 50 ?? ?? ?? ?? ?? ?? 56 ?? ?? ?? ?? ?? 83 C4 04 8B F0")
        .expect("get_build_number_fn signature not found")
        .add(0x14)
        .deref_rel()
        .expect("get_build_number_fn cannot follow call branch.");
    unsafe { std::mem::transmute(result_fn.value()) }
});

pub fn get_build_number() -> u32 {
    GET_BUILD_NUMBER_FN()
}

 