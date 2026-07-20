
use crate::memory::{Address,ProcessModule};
use crate::{gw};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};



pub struct MissionConstData {
    mapid: u32,
    ptr: Address
}

impl MissionConstData {

    pub fn get(mapid: u32) -> Self {
        static FUNC: LazyLock<extern "cdecl" fn(u32) -> usize> = LazyLock::new(|| {
            let result_fn = ProcessModule::main()
                .expect("Cant get main module.")
                .find_pattern("55 8B EC 56 8B 75 08 81 FE 75 03 00 00")
                .expect("MissionConstData::get signature not found");
            unsafe { std::mem::transmute(result_fn.value()) }
        });
        Self{
            mapid: mapid,
            ptr: Address::at(FUNC(mapid))
        }
    }

    pub fn name_id(&self) -> u32 {
        unsafe {self.ptr.add(0x74).read::<u32>()}
    }
}


/// Cache of decoded map names, keyed by map id.
///
/// `decode_string_async` hands the result back through a plain `fn` callback
/// that can capture nothing, so the cache lives in a global the callback can
/// reach. `pending` tracks ids we've already kicked off a decode for, so
/// repeated `map_name` calls (e.g. once per frame while a run row is on screen)
/// don't spam duplicate requests.
struct MapNameCache {
    names: HashMap<u32, String>,
    pending: HashSet<u32>,
}

static MAP_NAMES: LazyLock<Mutex<MapNameCache>> = LazyLock::new(|| {
    Mutex::new(MapNameCache {
        names: HashMap::new(),
        pending: HashSet::new(),
    })
});

/// Look up the decoded name for `mapid`.
///
/// Returns `Some(name)` once it's been decoded and cached. The first time an id
/// is requested it returns `None` and kicks off an async decode in the
/// background; call again on a later frame to get the resolved name. Callers
/// should fall back to displaying the numeric id while this is `None`.
///
/// Must be called on the game's render/main thread — it invokes game functions.
pub fn map_name(mapid: u32) -> Option<String> {
    if mapid == 0 {
        return None;
    }

    // Fast path: already resolved.
    {
        let cache = MAP_NAMES.lock().unwrap();
        if let Some(name) = cache.names.get(&mapid) {
            return Some(name.clone());
        }
    }

    request_decode(mapid);
    None
}

/// Kick off a single async decode for `mapid` if one isn't already in flight
/// and the name isn't already cached.
fn request_decode(mapid: u32) {
    // Reserve the request under the lock, then release it before calling into
    // the game: `decode_string_async` may invoke our callback synchronously,
    // and `on_decoded` needs to take this same lock.
    {
        let mut cache = MAP_NAMES.lock().unwrap();
        if cache.names.contains_key(&mapid) || !cache.pending.insert(mapid) {
            return;
        }
    }

    let name_id = MissionConstData::get(mapid).name_id();
    // The game consumes the encoded buffer synchronously to start the decode;
    // only the callback is deferred, so this stack-local buffer is fine.
    let enc = gw::string::int_to_enc_string(name_id);
    gw::string::decode_string_async(&enc, on_decoded, mapid as usize);
}

/// Callback for `decode_string_async`. `arg` carries the `mapid` we passed in.
fn on_decoded(arg: usize, decoded: String) {
    let mapid = arg as u32;
    let mut cache = MAP_NAMES.lock().unwrap();
    cache.pending.remove(&mapid);
    cache.names.insert(mapid, decoded);
}