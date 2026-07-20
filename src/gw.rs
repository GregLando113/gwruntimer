
use rusqlite::Map;

use crate::memory::{Address,ProcessModule};
use std::{sync::LazyLock, time::Duration};

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

#[cfg(all(windows, target_arch = "x86"))]
#[inline]
// unsafe outside of gw render thread
 pub fn get_context_tls() -> Address {
    use core::arch::asm;
    let mut ctx :usize = 0;

    unsafe {
        asm!(
            "mov eax, fs:[0x2C]",
            "mov eax, [eax]",
            "mov eax, [eax+8]",
            "mov {}, eax",
            out(reg) ctx,
            options(nostack, readonly, preserves_flags),
        );
    }
    Address { addr: ctx }
 }

pub struct InstanceUpTimePtr {
    pp : Address
}

impl InstanceUpTimePtr {

     pub fn get_raw(&self) -> u32 {
        unsafe { self.pp.read::<u32>() }
    }

    pub fn get(&self) -> Duration {
        Duration::from_millis(self.get_raw().into())
    }

}

 pub fn get_instance_up_time_ptr() -> InstanceUpTimePtr {
    InstanceUpTimePtr { pp: get_context_tls()
    .add(0x8).safe_deref().expect("AgentCtx is null")
    .add(0x1A8) }
 }

 pub struct MapDataPtr {
    pp : Address
 }

 impl MapDataPtr {

    pub fn load_state(&self) -> u32 {
        unsafe { self.pp.read::<u32>() }
    }

    pub fn is_loaded(&self) -> bool {
        self.load_state() == 0xC8
    }

    pub fn map_id(&self) -> u32 {
        unsafe { self.pp.add(0x8).read::<u32>() }
    }

    
    pub fn is_explorable(&self) -> bool {
        unsafe { self.pp.add(0xC).read::<u32>() != 0 }
    }

 }


 pub fn get_map_data_ptr() -> MapDataPtr {
    MapDataPtr { pp: get_context_tls()
    .add(0x44).safe_deref().expect("MissionCtx is null")
    .add(0x190) }
 }



 pub struct CharDataPtr {
    pp: Address
 }


 impl CharDataPtr {

    pub fn uuid(&self) -> [u8; 16] {
        let data: [u8; 16] = unsafe { self.pp.read() };
        data
    }

    pub fn name(&self) -> String {
        let data: [u8; 40] = unsafe { self.pp.add(16).read() };
        String::from_utf16le(&data).expect("error getting charname :/")
    }
    
 }

 
 
pub fn get_char_data_ptr() -> CharDataPtr {
    CharDataPtr { pp: get_context_tls()
    .add(0x44).safe_deref().expect("MissionCtx is null")
    .add(0x64) }
 }


 pub struct ControlledPlayer {
    pp: Address
 }

 impl ControlledPlayer {

    pub fn deref(&self) -> Address {
        self.pp.safe_deref().expect("ControlledPlayer pp nil")
    }
     
    pub fn agent_id(&self) -> u32 {
        unsafe { self.deref().add(0x14).read::<u32>() }
    }

    pub fn is_moving(&self) -> bool {
        unsafe { self.deref().add(0x68).read::<u32>() > 0 }
    }
 }

 pub fn get_controlled_player() -> ControlledPlayer {
    ControlledPlayer { pp: get_context_tls()
    .add(0x2c).safe_deref().expect("CharCtx is null")
    .add(0x680) }
 }