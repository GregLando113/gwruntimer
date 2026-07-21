
use crate::memory::{Address,ProcessModule};
use std::sync::LazyLock;

static DECODE_STRING_ASYNC_FUNC: LazyLock<
    extern "cdecl" fn(*const u16, extern "cdecl" fn(usize, *const u16), usize),
> = LazyLock::new(|| {
    let result_fn = ProcessModule::main()
        .expect("Cant get main module.")
        .find_pattern("0f b7 07 25 ff 7f ff ff 3d 00 01 00 00 73 14")
        .expect("decode_string_async signature not found")
        .sub(0x3c8f79 - 0x3c8f40);
    unsafe { std::mem::transmute(result_fn.value()) }
});

pub fn int_to_enc_string(num: u32) -> Vec<u16> {
    // Each word holds a base-BASE digit offset by WORD_VALUE_BASE. Non-final
    // words also set WORD_BIT_MORE to mark that more words follow. BASE is the
    // count of representable digit values per word (0x100..=0x7fff).
    const WORD_BIT_MORE: u16 = 0x8000;
    const WORD_VALUE_BASE: u16 = 0x100;
    const BASE: u64 = (WORD_BIT_MORE - WORD_VALUE_BASE) as u64; // 0x7f00

    let n = num as u64; // 64-bit math, matching the (ulonglong) casts in the C
    let mut out: Vec<u16> = Vec::new();

    // Multibyte case: emit every digit except the least-significant one, each
    // flagged with WORD_BIT_MORE.
    if n >= BASE {
        // Smallest place value whose quotient fits in one digit (do-while in C).
        let mut place = BASE;
        while n / place >= BASE {
            place *= BASE;
        }
        // Most-significant digit first.
        loop {
            let digit = ((n / place) % BASE) as u16;
            out.push(WORD_BIT_MORE | (WORD_VALUE_BASE + digit));
            place /= BASE;
            if place == 1 {
                break;
            }
        }
    }

    // Least-significant digit: no WORD_BIT_MORE, so it terminates the value.
    out.push(WORD_VALUE_BASE + (n % BASE) as u16);

    // Null-terminate so the buffer can be handed to decode_string_async as a
    // *const u16 (GW encoded strings are 0x0000-terminated).
    out.push(0);

    out
}

struct DecodeCtx {
    arg: usize,
    callback: fn(arg: usize, decoded: String),
}

pub fn decode_string_async(
    enc_string: &Vec<u16>,
    callback: fn(arg: usize, decoded: String),
    arg: usize,
) {


    // Hand ownership of the context to C as a raw integer.
    let ctx = Box::new(DecodeCtx { arg, callback });
    let ctx_ptr = Box::into_raw(ctx) as usize;

    // Non-capturing trampoline: reclaims the box, decodes, calls the real callback.
    extern "cdecl" fn cb_mapper(ctx_ptr: usize, decoded: *const u16) {
        // SAFETY: ctx_ptr is the pointer we leaked above; the game calls us
        // exactly once, so reclaiming (and freeing) here is correct.
        let ctx = unsafe { Box::from_raw(ctx_ptr as *mut DecodeCtx) };

        let s = if decoded.is_null() {
            String::new()
        } else {
            unsafe {
                let mut len = 0usize;
                while *decoded.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(decoded, len);
                String::from_utf16_lossy(slice)
            }
        };

        (ctx.callback)(ctx.arg, s);
        // `ctx` (the Box) drops here → the DecodeCtx is freed.
    }

    DECODE_STRING_ASYNC_FUNC(enc_string.as_ptr(), cb_mapper, ctx_ptr);
}