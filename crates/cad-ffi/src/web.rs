//! WebAssembly entry points, for the browser workbench (`apps/web`).
//!
//! Built for `wasm32-unknown-unknown` with nothing but the standard
//! library: the module imports nothing, and the page's own script
//! (`apps/web/core.js`) is all the glue there is. Only numbers cross the
//! boundary. Bytes go in through memory the page asks for with
//! `cad_alloc` and gives back with `cad_free`; whatever a call produces
//! (JSON, a display list, a file) waits in one result buffer that the
//! page reads through `cad_result_ptr` and `cad_result_len` before the
//! next call.
//!
//! Calls that can fail return 0 on failure, with the message for the
//! user in the result buffer.
//!
//! The browser runs the core with `panic=abort`, so a panic (a bug: the
//! core reports bad input instead of panicking) stops the module. The
//! hook keeps the message, which the page reads with `cad_panic_message`
//! before it starts a fresh module.

use crate::session::Session;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The allocator, counting what is held. Linear memory only ever grows,
/// so its size says how much was once needed; this says how much is in
/// use now.
struct Counting;

static HEAP: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is passed straight to the system allocator with the
// same arguments; the count is bookkeeping only.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: as for the trait; the caller's contract is passed on.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            HEAP.fetch_add(l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            HEAP.fetch_add(l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: as for the trait.
        unsafe { System.dealloc(p, l) };
        HEAP.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        // SAFETY: as for the trait.
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            HEAP.fetch_sub(l.size(), Ordering::Relaxed);
            HEAP.fetch_add(size, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn heap_bytes() -> usize {
    HEAP.load(Ordering::Relaxed)
}

/// Above this, history is let go and new drawings and edits are refused
/// (see `Session::with_heap_limit`): well short of 2 GiB, where browsers
/// start to fail, and of the 4 GiB a 32-bit module can address.
const HEAP_LIMIT: usize = 1 << 30;

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::with_heap_limit(heap_bytes, HEAP_LIMIT));
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_result(bytes: Vec<u8>) {
    RESULT.with(|r| *r.borrow_mut() = bytes);
}

/// Store a call's outcome; 1 for success, 0 for failure.
fn finish(r: Result<Vec<u8>, String>) -> i32 {
    match r {
        Ok(b) => {
            set_result(b);
            1
        }
        Err(e) => {
            set_result(e.into_bytes());
            0
        }
    }
}

fn hook() {
    use std::sync::Once;
    static SET: Once = Once::new();
    SET.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            // `try_` so a panic while the message is borrowed cannot
            // panic again.
            let _ = PANIC.try_with(|p| {
                if let Ok(mut p) = p.try_borrow_mut() {
                    *p = msg;
                }
            });
        }));
    });
}

/// Bytes the page put in memory for this call.
///
/// # Safety
///
/// `ptr` must point to `len` initialised bytes the page wrote into memory
/// it got from `cad_alloc`, which stays allocated for the call.
unsafe fn input<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    // SAFETY: from this function's contract.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Text the page passed; invalid UTF-8 is replaced.
///
/// # Safety
///
/// As for [`input`].
unsafe fn text(ptr: *const u8, len: usize) -> String {
    // SAFETY: from this function's contract.
    String::from_utf8_lossy(unsafe { input(ptr, len) }).into_owned()
}

#[unsafe(no_mangle)]
pub extern "C" fn cad_abi_version() -> i32 {
    crate::ABI_VERSION
}

/// `len` bytes for the page to write into; give them back with
/// `cad_free`. Null when `len` is 0 or too large.
#[unsafe(no_mangle)]
pub extern "C" fn cad_alloc(len: usize) -> *mut u8 {
    hook();
    let Ok(layout) = std::alloc::Layout::array::<u8>(len) else {
        return std::ptr::null_mut();
    };
    if len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: the layout has a non-zero size.
    unsafe { std::alloc::alloc(layout) }
}

/// Give back memory from `cad_alloc`.
///
/// # Safety
///
/// `ptr` and `len` must be exactly what `cad_alloc` returned and was
/// asked for, given back once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cad_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = std::alloc::Layout::array::<u8>(len) {
        // SAFETY: from this function's contract: allocated by `cad_alloc`
        // with this same layout.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// Bytes the core holds now (its allocations), for the page's memory
/// warning.
#[unsafe(no_mangle)]
pub extern "C" fn cad_heap_bytes() -> usize {
    heap_bytes()
}

#[unsafe(no_mangle)]
pub extern "C" fn cad_result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn cad_result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

/// The program's name and version (`cad_core::PRODUCT_NAME`, a space,
/// `cad_core::VERSION`), in the result.
#[unsafe(no_mangle)]
pub extern "C" fn cad_version() -> i32 {
    finish(Ok(format!("{} {}", cad_core::PRODUCT_NAME, cad_core::VERSION).into_bytes()))
}

/// The message of the panic that stopped the module, in the result.
#[unsafe(no_mangle)]
pub extern "C" fn cad_panic_message() -> i32 {
    let m = PANIC.with(|p| p.borrow().clone());
    finish(Ok(m.into_bytes()))
}

/// Open a drawing from bytes: its id, or 0.
///
/// # Safety
///
/// `ptr` and `len` describe bytes as for [`input`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cad_open(ptr: *const u8, len: usize) -> u32 {
    hook();
    // SAFETY: from this function's contract.
    let bytes = unsafe { input(ptr, len) };
    match SESSION.with(|s| s.borrow_mut().open(bytes)) {
        Ok(id) => id,
        Err(e) => {
            set_result(e.into_bytes());
            0
        }
    }
}

/// An id, or 0 with the message in the result.
fn id_or_message(r: Result<u32, String>) -> u32 {
    r.unwrap_or_else(|e| {
        set_result(e.into_bytes());
        0
    })
}

/// Start a drawing: imperial (inches) when `imperial` is not 0,
/// otherwise metric (millimetres). Its id, or 0 when memory is short.
#[unsafe(no_mangle)]
pub extern "C" fn cad_new(imperial: i32) -> u32 {
    hook();
    id_or_message(SESSION.with(|s| s.borrow_mut().new_drawing(imperial != 0)))
}

/// Open the sample plate (the drawing in the workspace mockups). Its id,
/// or 0 when memory is short.
#[unsafe(no_mangle)]
pub extern "C" fn cad_sample() -> u32 {
    hook();
    id_or_message(SESSION.with(|s| s.borrow_mut().sample()))
}

#[unsafe(no_mangle)]
pub extern "C" fn cad_close(id: u32) {
    SESSION.with(|s| s.borrow_mut().close(id));
}

/// The drawing's units, layers, history and summary as JSON.
#[unsafe(no_mangle)]
pub extern "C" fn cad_info(id: u32) -> i32 {
    hook();
    finish(SESSION.with(|s| s.borrow_mut().info(id)).map(String::into_bytes))
}

/// Model space as a display list.
#[unsafe(no_mangle)]
pub extern "C" fn cad_display(id: u32) -> i32 {
    hook();
    finish(SESSION.with(|s| s.borrow_mut().display(id)))
}

/// What changed in the display since the page last read it: a patch of
/// the entities that changed, or the whole display list (see
/// `Session::display_changes`).
#[unsafe(no_mangle)]
pub extern "C" fn cad_display_changes(id: u32) -> i32 {
    hook();
    finish(SESSION.with(|s| s.borrow_mut().display_changes(id)))
}

/// Run a command line; its JSON answer.
///
/// # Safety
///
/// `ptr` and `len` describe the UTF-8 command as for [`input`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cad_exec(id: u32, ptr: *const u8, len: usize) -> i32 {
    hook();
    // SAFETY: from this function's contract.
    let line = unsafe { text(ptr, len) };
    finish(SESSION.with(|s| s.borrow_mut().exec(id, &line)).map(String::into_bytes))
}

/// The drawing as a file in `format` (see `session::SAVE_FORMATS`),
/// stamped as saved by `user` at `unix_seconds`. Notes on anything the
/// format could not hold are kept for `cad_save_notes`.
///
/// # Safety
///
/// Both strings are described as for [`input`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cad_save(
    id: u32,
    format_ptr: *const u8,
    format_len: usize,
    unix_seconds: f64,
    user_ptr: *const u8,
    user_len: usize,
) -> i32 {
    hook();
    // SAFETY: from this function's contract.
    let (format, user) = unsafe { (text(format_ptr, format_len), text(user_ptr, user_len)) };
    let r = SESSION.with(|s| s.borrow_mut().save(id, &format, unix_seconds as i64, &user));
    match r {
        Ok((bytes, notes)) => {
            NOTES.with(|n| *n.borrow_mut() = notes.join("\n"));
            finish(Ok(bytes))
        }
        Err(e) => finish(Err(e)),
    }
}

thread_local! {
    static NOTES: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The notes from the last `cad_save`, one per line, in the result.
#[unsafe(no_mangle)]
pub extern "C" fn cad_save_notes() -> i32 {
    finish(Ok(NOTES.with(|n| n.borrow().clone()).into_bytes()))
}
