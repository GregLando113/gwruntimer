
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Threading::GetCurrentProcess;

use std::ops::{Add, Sub};


#[derive(Debug)]
pub enum MemoryError {
    ReadFailed(u32),
    WriteFailed(u32),
    #[allow(dead_code)]
    Win32Error(u32),
    ScanNotFound,
    MemoryOutOfBounds,
    PEHeaderInvalid(u16),
    InvalidModule(String)
}


#[derive(Clone,Copy)]
pub struct Address(usize);

pub type AddressResult = Result<Address,MemoryError>;


impl Address {
    
    pub fn at(addr: usize) -> Self { Self(addr) }

    pub fn value(&self) -> usize { self.0 }

    pub fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }

    pub fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }

    pub fn add_addr(&self, offset: Self) -> Self {
        Self(self.0 + offset.0)
    }

    pub fn sub_addr(&self, offset: Self) -> Self {
        Self(self.0 - offset.0)
    }

    pub unsafe fn deref(&self) -> Self {
       Self(unsafe { std::ptr::read(self.0 as *const usize) })
    }

    pub unsafe fn as_ref<'a, T>(&self) -> &'a T {
       unsafe { &*(self.0 as *const T) }
    }

    pub unsafe fn as_mut_ref<'a, T>(&self) -> &'a mut T {
       unsafe { &mut *(self.0 as *mut T) }
    }

    pub unsafe fn read<T: Copy>(&self) -> T {
        unsafe { std::ptr::read(self.0 as *const T) }
    }

    pub unsafe fn write<T>(&self, value: T) {
        unsafe { std::ptr::write(self.0 as *mut T, value) };
    }

    // Ensure the deref will be without exception at additional syscall cost

    pub fn safe_read<T: Copy>(&self) -> Result<T, MemoryError> {
        unsafe {
            let mut value: T = std::mem::zeroed();
            let mut bytes_read = 0;

            let result = ReadProcessMemory(
                GetCurrentProcess(),
                self.0 as *const core::ffi::c_void,
                &mut value as *mut T as *mut core::ffi::c_void,
                std::mem::size_of::<T>(),
                Some(&mut bytes_read),
            );

            if result.is_err() {
                return Err(MemoryError::ReadFailed(windows::Win32::Foundation::GetLastError().0));
            }

            if bytes_read != std::mem::size_of::<T>() {
                return Err(MemoryError::ReadFailed(0)); // Incomplete read
            }

            Ok(value)
        }
    }

    pub fn safe_write<T>(&self, value: T) -> Result<(), MemoryError> {
        unsafe {
            let mut bytes_written = 0;

            let result = WriteProcessMemory(
                GetCurrentProcess(),
                self.0 as *const core::ffi::c_void,
                &value as *const T as *const core::ffi::c_void,
                std::mem::size_of::<T>(),
                Some(&mut bytes_written),
            );

            if result.is_err() {
                return Err(MemoryError::WriteFailed(windows::Win32::Foundation::GetLastError().0));
            }

            if bytes_written != std::mem::size_of::<T>() {
                return Err(MemoryError::WriteFailed(0)); // Incomplete write
            }

            Ok(())
        }
    }

    pub fn within_module(&self, process_module: Option<&ProcessModule>) -> Result<(), MemoryError> {
        let my_module = match process_module {
            Some(module) => module,
            None => &ProcessModule::main()?
        };
        if self.0 >= my_module.base_.0 
        && self.0 < my_module.base_.0 + my_module.size_ {
            Ok(())
        }
        else {
            Err(MemoryError::MemoryOutOfBounds)
        }
    }

    pub fn safe_deref(&self) -> Result<Self,MemoryError> {
        Ok(Self(self.safe_read::<usize>()?))
    }

    pub fn deref_rel(&self) -> Result<Self, MemoryError> {
        let new_addr = Self((self.0 as isize + self.safe_read::<i32>()? as isize + 4) as usize);
        new_addr.within_module(None)?;
        Ok(new_addr)
    }


}

impl Add for Address {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.add_addr(rhs)
    }
}

impl Sub for Address {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.sub_addr(rhs)
    }
}

impl Add<usize> for Address {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        self.add(rhs)
    }
}

impl Sub<usize> for Address {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
       self.sub(rhs)
    }
}



pub struct ProcessModule {
    base_: Address,
    size_: usize
}


impl ProcessModule {

    pub fn base(&self) -> Address { self.base_ }
    pub fn size(&self) -> usize { self.size_ }

    pub fn from_name(module_name: Option<&str>) -> Result<Self, MemoryError> {
        let base = unsafe { Process::get_module_base(module_name)? };
        let size = unsafe { Process::get_module_size(base)? };
        
        Ok( Self { base_: Address(base), size_: size as usize })
        
    }

    pub fn main() -> Result<Self, MemoryError> {
        Self::from_name(None)
    }

    pub fn find_pattern(&self, pattern: &str) -> Result<Address, MemoryError> {
        let (pattern_bytes, mask) = Self::parse_pattern(pattern);

        if pattern_bytes.is_empty() {
            return Err(MemoryError::ScanNotFound);
        }

        let scan_size = self.size_ - pattern_bytes.len() + 1;

        for i in 0..scan_size {
            let current_addr = self.base_.add(i);

            let mut found = true;
            for (j, &pattern_byte) in pattern_bytes.iter().enumerate() {
                if !mask[j] {
                    continue;
                }

                let addr = current_addr.add(j);
                match addr.safe_read::<u8>() {
                    Ok(byte) => {
                        if byte != pattern_byte {
                            found = false;
                            break;
                        }
                    }
                    Err(_) => {
                        found = false;
                        break;
                    }
                }
            }

            if found {
                //debug!("Found address for pattern \"{}\" at {:X}", pattern, current_addr.value());
                return Ok(current_addr);
            }
        }

        //debug!("Pattern \"{}\" not found", pattern);
        Err(MemoryError::ScanNotFound)
    }

    fn parse_pattern(pattern: &str) -> (Vec<u8>, Vec<bool>) {
        let parts: Vec<&str> = pattern.split_whitespace().collect();
        let mut bytes = Vec::new();
        let mut mask = Vec::new();

        for part in parts {
            if part == "?" || part == "??" {
                bytes.push(0);
                mask.push(false);
            } else if let Ok(byte) = u8::from_str_radix(part, 16) {
                bytes.push(byte);
                mask.push(true);
            }
        }

        (bytes, mask)
    }

}



pub struct Process {}

#[cfg(windows)]
impl Process {

    unsafe fn get_module_base(module_name: Option<&str>) -> Result<usize, MemoryError> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleA;
        use windows::core::PCSTR;

        let handle_result = match module_name {
            Some(name) => {
                let mut name_bytes = name.as_bytes().to_vec();
                name_bytes.push(0);
                unsafe { GetModuleHandleA(PCSTR::from_raw(name_bytes.as_ptr())) }
            }
            None => {
                // Get base of current process (your injected DLL's host)
                unsafe { GetModuleHandleA(PCSTR::null()) }
            }
        };
        match handle_result {
            Ok(h) => Ok(h.0 as usize),
            Err(_) => Err(MemoryError::InvalidModule(module_name.unwrap_or("Main").to_string()))
        }
    }

    unsafe fn get_module_size(base: usize) -> Result<u32, MemoryError> {
        // Manual PE header parsing since the structures aren't easily accessible in the windows crate
        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct IMAGE_DOS_HEADER {
            e_magic: u16,
            _padding: [u8; 58],
            e_lfanew: i32,
        }

        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct IMAGE_NT_HEADERS32 {
            Signature: u32,
            _FileHeader: [u8; 20],
            OptionalHeader: IMAGE_OPTIONAL_HEADER32,
        }

        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct IMAGE_OPTIONAL_HEADER32 {
            Magic: u16,
            _padding: [u8; 54],
            SizeOfImage: u32,
            // ... rest of the structure
        }

        unsafe {
            let dos_header = &*(base as *const IMAGE_DOS_HEADER);
            if dos_header.e_magic != 0x5A4D {
                return Err(MemoryError::PEHeaderInvalid(dos_header.e_magic)); // Not a valid PE
            }

            let nt_headers = &*((base + dos_header.e_lfanew as usize) as *const IMAGE_NT_HEADERS32);
            Ok(nt_headers.OptionalHeader.SizeOfImage)
        }
    }


}


#[cfg(unix)]
impl Process {

    pub unsafe fn get_module_base(module_name: Option<&str>) -> Option<usize> {
        // For Linux/Unix, parse /proc/self/maps
        use std::fs;
        
        let maps = fs::read_to_string("/proc/self/maps").ok()?;
        
        for line in maps.lines() {
            if let Some(name) = module_name {
                if line.contains(name) {
                    let addr_str = line.split('-').next()?;
                    return usize::from_str_radix(addr_str, 16).ok();
                }
            } else if line.contains("[exe]") || line.contains("r-xp") {
                let addr_str = line.split('-').next()?;
                return usize::from_str_radix(addr_str, 16).ok();
            }
        }
        None
    }
}



