
use crate::memory::{Address,ProcessModule};
use std::sync::LazyLock;



pub struct MissionConstData(Address);

impl MissionConstData {

    pub fn get(mapid: u32) -> Self {
        static FUNC: LazyLock<extern "cdecl" fn(u32) -> usize> = LazyLock::new(|| {
            let result_fn = ProcessModule::main()
                .expect("Cant get main module.")
                .find_pattern("55 8B EC 56 8B 75 08 81 FE 75 03 00 00")
                .expect("MissionConstData::get signature not found");
            unsafe { std::mem::transmute(result_fn.value()) }
        });
        Self(Address::at(FUNC(mapid)))
    }

    pub fn name_id(&self) -> u32 {
        unsafe {self.0.add(0x74).read::<u32>()}
    }
}