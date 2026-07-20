
use crate::memory::{Address,ProcessModule};
use std::{sync::LazyLock, time::Duration};

pub fn get_build_number() -> u32 {
    static GET_BUILD_NUMBER_FN: LazyLock<extern "cdecl" fn() -> u32> = LazyLock::new(|| {
        let result_fn = ProcessModule::main()
            .expect("Cant get main module.")
            .find_pattern("57 50 ?? ?? ?? ?? ?? ?? 56 ?? ?? ?? ?? ?? 83 C4 04 8B F0")
            .expect("get_build_number_fn signature not found")
            .add(0x14)
            .deref_rel()
            .expect("get_build_number_fn cannot follow call branch.");
        unsafe { std::mem::transmute(result_fn.value()) }
    });
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
    Address::at(ctx)
 }

pub struct InstanceUpTimePtr(Address);

impl InstanceUpTimePtr {

    pub fn current() -> Self {
        Self(get_context_tls()
        .add(0x8).safe_deref().expect("AgentCtx is null")
        .add(0x1A8))
    }

     pub fn get_raw(&self) -> u32 {
        unsafe { self.0.read::<u32>() }
    }

    pub fn get(&self) -> Duration {
        Duration::from_millis(self.get_raw().into())
    }

}

 pub struct MapDataPtr(Address);

 impl MapDataPtr {

     pub fn current() -> Self {
        Self(get_context_tls()
        .add(0x44).safe_deref().expect("MissionCtx is null")
        .add(0x190))
    }

    pub fn load_state(&self) -> u32 {
        unsafe { self.0.read::<u32>() }
    }

    pub fn is_loaded(&self) -> bool {
        self.load_state() == 0xC8
    }

    pub fn map_id(&self) -> u32 {
        unsafe { self.0.add(0x8).read::<u32>() }
    }


    pub fn is_explorable(&self) -> bool {
        unsafe { self.0.add(0xC).read::<u32>() != 0 }
    }

 }






 pub struct CharDataPtr(Address);


 impl CharDataPtr {

    pub fn current() -> Self {
        Self(get_context_tls()
        .add(0x44).safe_deref().expect("MissionCtx is null")
        .add(0x64))
    }

    pub fn uuid(&self) -> [u8; 16] {
        let data: [u8; 16] = unsafe { self.0.read() };
        data
    }

    pub fn name(&self) -> String {
        let data: [u8; 40] = unsafe { self.0.add(16).read() };
        let units: Vec<u16> = data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

 }






 pub struct ControlledPlayer(Address);

 impl ControlledPlayer {

    pub fn current() -> Self {
        Self(get_context_tls()
        .add(0x2c).safe_deref().expect("CharCtx is null")
        .add(0x680))
    }

    pub fn deref(&self) -> Address {
        self.0.safe_deref().expect("ControlledPlayer pp nil")
    }
     
    pub fn agent_id(&self) -> u32 {
        unsafe { self.deref().add(0x14).read::<u32>() }
    }

    pub fn is_moving(&self) -> bool {
        unsafe { self.deref().add(0x68).read::<u32>() > 0 }
    }
 }

