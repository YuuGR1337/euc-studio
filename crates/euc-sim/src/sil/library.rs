//! The C ABI of `libeuc_sil` (`firmware/ports/sim/include/euc/sil.h`, ABI 2) and loading one
//! instance of it.
//!
//! The library's static state is the firmware's RAM, so every power-on needs a fresh instance. The
//! dynamic loader hands back the instance already loaded for a path, so each [`Instance`] loads its
//! own copy of the file from a temporary path, which it removes at once (the mapping stays).

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use libloading::Library;

/// The ABI version this engine speaks (`EUC_SIL_ABI_VERSION`).
pub(crate) const ABI_VERSION: u32 = 2;

/// Host call answers (`EUC_SIL_GO_ON`, `EUC_SIL_STOP`); a value of 0 or more names a context.
pub(crate) const GO_ON: i32 = -1;
pub(crate) const STOP: i32 = -2;

/// `euc_sil_run()` results.
pub(crate) const RUN_STOPPED: u32 = 0;

/// Profiles of `euc_sil_quirk_profile_mask()`.
pub(crate) const PROFILE_COMPAT: u32 = 0;
pub(crate) const PROFILE_CLEAN: u32 = 1;

/// `struct euc_sil_host`: the board, as the engine gives it to the firmware.
#[repr(C)]
pub(crate) struct HostTable {
    pub(crate) context: *mut c_void,
    pub(crate) read: unsafe extern "C" fn(*mut c_void, u32, u32, *mut u32) -> i32,
    pub(crate) write: unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> i32,
    pub(crate) spin: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub(crate) returned: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub(crate) load: unsafe extern "C" fn(*mut c_void, u32, *mut u8, u32),
    pub(crate) store: unsafe extern "C" fn(*mut c_void, u32, *const u8, u32),
    pub(crate) fault: unsafe extern "C" fn(*mut c_void, *const c_char),
}

/// The library's functions, resolved once per instance.
#[derive(Clone, Copy)]
pub(crate) struct Api {
    pub(crate) quirk_count: unsafe extern "C" fn() -> u32,
    pub(crate) quirk_name: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) quirk_summary: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) quirk_profile_mask: unsafe extern "C" fn(u32) -> u64,
    pub(crate) quirk_set_mask: unsafe extern "C" fn(u64),
    pub(crate) attach: unsafe extern "C" fn(*const HostTable) -> bool,
    pub(crate) run: unsafe extern "C" fn() -> u32,
    pub(crate) context_count: unsafe extern "C" fn() -> u32,
    pub(crate) context_irq: unsafe extern "C" fn(u32) -> i32,
    pub(crate) context_name: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) probe_count: unsafe extern "C" fn() -> u32,
    pub(crate) probe_name: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) probe_kind: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) probe_role: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) probe_note: unsafe extern "C" fn(u32) -> *const c_char,
    pub(crate) probe_offset: unsafe extern "C" fn(u32) -> u32,
    pub(crate) probe_size: unsafe extern "C" fn(u32) -> u32,
    pub(crate) state: unsafe extern "C" fn() -> *const c_void,
    pub(crate) state_bytes: unsafe extern "C" fn() -> u32,
}

/// One loaded copy of the library: one firmware instance.
pub(crate) struct Instance {
    pub(crate) api: Api,
    /// Kept loaded while `api` is in use; dropped last.
    _library: Library,
}

/// Distinguishes the temporary copies of one process.
static COPIES: AtomicU64 = AtomicU64::new(0);

impl Instance {
    /// Loads a fresh copy of the library at `path` and checks its ABI version.
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let copy = temporary_copy(path)?;
        // SAFETY: loading runs the library's initialisers. libeuc_sil has none beyond the C runtime's
        // zeroing of its statics; the file is the firmware build the caller chose to run.
        let library = unsafe { Library::new(&copy) };
        let _ = std::fs::remove_file(&copy);
        let library = library.map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: the symbol types are those of `euc/sil.h` for ABI 2, which the version check below
        // confirms before any other function is called.
        let api = unsafe { resolve(&library) }.map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self { api, _library: library })
    }

    /// A NUL-terminated string the library returned, or `None` for NULL. It borrows the instance
    /// because the string lives in the library.
    #[allow(clippy::unused_self)]
    pub(crate) fn text(&self, pointer: *const c_char) -> Option<String> {
        // SAFETY: the library returns NULL or pointers to its own static NUL-terminated strings,
        // which live as long as `self`.
        (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
    }
}

/// Resolves every function of ABI 2.
///
/// # Safety
///
/// `library` must be a `libeuc_sil` build; the version is checked here before anything else.
unsafe fn resolve(library: &Library) -> Result<Api, String> {
    macro_rules! get {
        ($name:literal) => {{
            // SAFETY: the caller guarantees the library's ABI; the type is that of `euc/sil.h`.
            let symbol = unsafe { library.get(concat!($name, "\0").as_bytes()) }
                .map_err(|e| format!("no {}: {e} (not a libeuc_sil build?)", $name))?;
            *symbol
        }};
    }
    let version: unsafe extern "C" fn() -> u32 = get!("euc_sil_abi_version");
    // SAFETY: every libeuc_sil exports this function with this type since ABI 1.
    let version = unsafe { version() };
    if version != ABI_VERSION {
        return Err(format!("SIL ABI {version}, but this engine speaks ABI {ABI_VERSION}: rebuild the library"));
    }
    Ok(Api {
        quirk_count: get!("euc_sil_quirk_count"),
        quirk_name: get!("euc_sil_quirk_name"),
        quirk_summary: get!("euc_sil_quirk_summary"),
        quirk_profile_mask: get!("euc_sil_quirk_profile_mask"),
        quirk_set_mask: get!("euc_sil_quirk_set_mask"),
        attach: get!("euc_sil_attach"),
        run: get!("euc_sil_run"),
        context_count: get!("euc_sil_context_count"),
        context_irq: get!("euc_sil_context_irq"),
        context_name: get!("euc_sil_context_name"),
        probe_count: get!("euc_sil_probe_count"),
        probe_name: get!("euc_sil_probe_name"),
        probe_kind: get!("euc_sil_probe_kind"),
        probe_role: get!("euc_sil_probe_role"),
        probe_note: get!("euc_sil_probe_note"),
        probe_offset: get!("euc_sil_probe_offset"),
        probe_size: get!("euc_sil_probe_size"),
        state: get!("euc_sil_state"),
        state_bytes: get!("euc_sil_state_bytes"),
    })
}

/// Copies `path` to a fresh name in the temporary directory.
fn temporary_copy(path: &Path) -> Result<PathBuf, String> {
    let n = COPIES.fetch_add(1, Ordering::Relaxed);
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("so");
    let copy = std::env::temp_dir().join(format!("euc-sil-{}-{n}.{extension}", std::process::id()));
    std::fs::copy(path, &copy).map_err(|e| format!("{}: cannot copy to {}: {e}", path.display(), copy.display()))?;
    Ok(copy)
}
