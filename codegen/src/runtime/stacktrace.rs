//! Captured stack traces — bounded native frame-pointer walk at throw time.
//!
//! Aster does not unwind: errors propagate by return path (`!` checks the error
//! slot). So at the moment of `throw` the native stack is fully intact and the
//! complete trace is walkable exactly once, right there in
//! `aster_error_set_typed`. This module owns three responsibilities:
//!
//! 1. Recording the current execution context's native stack bounds (main
//!    thread + blocking-pool threads lazily / at thread start; green threads at
//!    context-switch-in with their exact mmap'd bounds), so a frame-pointer walk
//!    can never read past the stack.
//! 2. Walking the frame-pointer chain from the throw site up to those bounds,
//!    once, capturing raw return-address PCs. The happy path (no throw) does no
//!    bookkeeping at all.
//! 3. Materializing the captured PCs as a `List[Frame]` when `error.trace()` is
//!    called. Frames that resolve in the Aster function table become real
//!    frames; contiguous unresolved runtime-internal frames collapse into a
//!    single `[runtime]` marker.
//!
//! Symbolization tables (JIT address-range registration, AOT data-section
//! emission, and PC-offset -> source-span tables) are layered on top of
//! `resolve_frame`; until a table is registered every PC resolves as runtime
//! and the whole trace collapses to one `[runtime]` frame.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

/// Hard cap on captured frames so a corrupt chain can never loop unbounded.
const MAX_TRACE_FRAMES: usize = 256;

// ---------------------------------------------------------------------------
// Symbolization: a shared sorted address-range -> function table.
//
// Both backends populate the SAME structure so AOT and JIT resolve identically:
//   * JIT registers each finalized function's `[addr, addr+len)` range directly
//     (in-process, as functions finalize).
//   * AOT emits a self-contained data section (function address via relocation,
//     size, name/file string indices, and a PC-offset -> line table) that the
//     runtime walks into this same structure at program init.
//
// No dladdr, no platform symbol tables: resolution is a binary search on the
// captured PC into ranges the compiler emitted.
// ---------------------------------------------------------------------------

/// One resolvable Aster function: its native address range, name, source file,
/// and a PC-offset -> line table (sorted by offset). An empty `lines` table
/// falls back to `def_line` (the function's definition line).
struct FuncSym {
    start: usize,
    len: usize,
    name: String,
    file: String,
    def_line: i64,
    /// `(code_offset_from_start, line)` pairs, sorted by offset.
    lines: Vec<(u32, u32)>,
}

struct SymTable {
    entries: Vec<FuncSym>,
    sorted: bool,
}

static SYMBOLS: Mutex<SymTable> = Mutex::new(SymTable {
    entries: Vec::new(),
    sorted: true,
});

/// Register one function's symbolization record. Called by the JIT as each
/// function finalizes, and by the AOT init walker for each data-section entry.
pub(crate) fn register_function(
    start: usize,
    len: usize,
    name: String,
    file: String,
    def_line: i64,
    lines: Vec<(u32, u32)>,
) {
    if start == 0 || len == 0 {
        return;
    }
    let mut tbl = SYMBOLS.lock().unwrap();
    tbl.entries.push(FuncSym {
        start,
        len,
        name,
        file,
        def_line,
        lines,
    });
    tbl.sorted = false;
}

impl SymTable {
    #[cfg(test)]
    fn from_entries(entries: Vec<FuncSym>) -> Self {
        SymTable {
            entries,
            sorted: false,
        }
    }

    /// Resolve `pc` to `(function, file, line)`, or `None` if it lands in no
    /// registered range. Sorts lazily on first lookup after a registration.
    fn resolve(&mut self, pc: usize) -> Option<(String, String, i64)> {
        if self.entries.is_empty() {
            return None;
        }
        if !self.sorted {
            self.entries.sort_by_key(|e| e.start);
            self.sorted = true;
        }
        // Largest entry whose start <= pc.
        let idx = match self.entries.binary_search_by(|e| e.start.cmp(&pc)) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let sym = &self.entries[idx];
        if pc < sym.start || pc >= sym.start + sym.len {
            return None;
        }
        let line = line_for_offset(sym, pc);
        Some((sym.name.clone(), sym.file.clone(), line))
    }
}

/// Resolve `pc` inside a `FuncSym`'s line table to a source line. The captured
/// PC is a return address (one past the call), so we resolve `pc - 1` to stay
/// inside the call instruction's source region.
fn line_for_offset(sym: &FuncSym, pc: usize) -> i64 {
    if sym.lines.is_empty() {
        return sym.def_line;
    }
    // Return address points just after the call; step back one byte so the
    // offset lands within the originating instruction's range.
    let off = (pc.saturating_sub(1)).saturating_sub(sym.start) as u32;
    // Largest entry whose offset is <= off.
    let idx = match sym.lines.binary_search_by(|(o, _)| o.cmp(&off)) {
        Ok(i) => i,
        Err(0) => return sym.def_line,
        Err(i) => i - 1,
    };
    sym.lines[idx].1 as i64
}

thread_local! {
    /// `(lo, hi)` native stack bounds for the current execution context.
    /// `lo` = lowest valid address, `hi` = one-past the highest. `(0, 0)` means
    /// "unknown" and disables walking (fail safe: an empty trace, never a crash).
    static STACK_BOUNDS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };

    /// Error pointer thrown most recently on this thread. Used only by the
    /// test-only trace inspector; the real store is the cross-thread map below.
    static LAST_THROWN: Cell<usize> = const { Cell::new(0) };
}

/// Captured traces keyed by the error object pointer, worst (deepest / throw
/// site) frame first. Stored globally (not thread-local) and keyed on the error
/// object so a trace captured on a green thread or blocking-pool worker survives
/// the task resolving on another thread, and so capture-once semantics hold:
/// a rethrow of the same object finds its trace already present.
static TRACES: LazyLock<Mutex<HashMap<usize, Vec<usize>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Live entry count in `TRACES`, kept in step with the map (both mutated under
/// the same lock). Lets `forget` skip the lock entirely on the common path — a
/// program that never throws keeps this at 0, so GC sweep pays only a relaxed
/// load per freed object, preserving the "zero steady-state cost" guarantee.
static TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Set the current context's stack bounds explicitly (used by the green-thread
/// scheduler with mmap'd stack bounds, and by worker threads for their own OS
/// stack).
pub(crate) fn set_stack_bounds(lo: usize, hi: usize) {
    STACK_BOUNDS.set((lo, hi));
}

/// Current `(lo, hi)` stack bounds for this context.
pub(crate) fn stack_bounds() -> (usize, usize) {
    STACK_BOUNDS.get()
}

/// Query the OS for the calling thread's native stack bounds.
///
/// Returns `(lo, hi)` on success. Best-effort: on any failure or unsupported
/// platform it returns `None` and callers leave bounds unknown (walking then
/// produces an empty trace rather than reading unmapped memory).
pub(crate) fn os_thread_bounds() -> Option<(usize, usize)> {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut stackaddr: *mut libc::c_void = std::ptr::null_mut();
        let mut stacksize: libc::size_t = 0;
        let rc = libc::pthread_attr_getstack(&attr, &mut stackaddr, &mut stacksize);
        libc::pthread_attr_destroy(&mut attr);
        if rc != 0 || stackaddr.is_null() || stacksize == 0 {
            return None;
        }
        let lo = stackaddr as usize;
        Some((lo, lo + stacksize))
    }
    #[cfg(target_os = "macos")]
    unsafe {
        let me = libc::pthread_self();
        // `pthread_get_stackaddr_np` returns the base (highest address).
        let hi = libc::pthread_get_stackaddr_np(me) as usize;
        let size = libc::pthread_get_stacksize_np(me) as usize;
        if hi == 0 || size == 0 || size > hi {
            return None;
        }
        Some((hi - size, hi))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Record the calling OS thread's stack bounds into this context. Called at
/// worker / blocking-pool thread start and lazily on the first throw from any
/// thread whose bounds are still unknown (covers the main thread at init).
pub(crate) fn record_os_thread_bounds() {
    if let Some((lo, hi)) = os_thread_bounds() {
        STACK_BOUNDS.set((lo, hi));
    }
}

/// Ensure some stack bounds are recorded for this context; if none are, fall
/// back to the OS thread's bounds.
fn ensure_bounds() {
    if STACK_BOUNDS.get().1 == 0 {
        record_os_thread_bounds();
    }
}

/// Read the current frame pointer (rbp / x29). Frame pointers are forced on for
/// both Aster code (Cranelift `preserve_frame_pointers`) and the runtime
/// (`force-frame-pointers` in the staticlib build), so this is always valid.
#[inline(always)]
fn current_frame_pointer() -> usize {
    let fp: usize;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rbp", out(reg) fp, options(nomem, nostack, preserves_flags));
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack, preserves_flags));
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        fp = 0;
    }
    fp
}

/// Walk the frame-pointer chain starting at `start_fp`, collecting return
/// addresses into `out` (cleared first), worst frame first. Every dereference is
/// bounds- and alignment-checked so the walk stays strictly inside
/// `[lo, hi)` and terminates.
fn walk_fp_chain(start_fp: usize, lo: usize, hi: usize, out: &mut Vec<usize>) {
    out.clear();
    if lo == 0 || hi <= lo {
        return;
    }
    let mut fp = start_fp;
    for _ in 0..MAX_TRACE_FRAMES {
        // Need to read [fp] (saved fp) and [fp+8] (return address).
        if fp < lo || fp & 0x7 != 0 {
            break;
        }
        match fp.checked_add(16) {
            Some(end) if end <= hi => {}
            _ => break,
        }
        // SAFETY: fp is 8-aligned and [fp, fp+16) lies within the mapped,
        // readable stack region [lo, hi).
        let saved_fp = unsafe { std::ptr::read(fp as *const usize) };
        let ret_addr = unsafe { std::ptr::read((fp + 8) as *const usize) };
        if ret_addr != 0 {
            out.push(ret_addr);
        }
        // The chain must move strictly upward (the stack grows down); anything
        // else is a corrupt or terminal frame.
        if saved_fp <= fp {
            break;
        }
        fp = saved_fp;
    }
}

/// Capture a fresh trace for `error_ptr`, unless it already carries one
/// (capture-once: rethrowing the same error value preserves its trace).
/// Called from `aster_error_set_typed` on the throw path only.
pub(crate) fn capture_for(error_ptr: i64) {
    let owner = error_ptr as usize;
    LAST_THROWN.set(owner);
    if owner == 0 {
        return;
    }
    if TRACES.lock().unwrap().contains_key(&owner) {
        // This error already carries a trace — preserve it (no recapture).
        return;
    }
    ensure_bounds();
    let (lo, hi) = STACK_BOUNDS.get();
    let start = current_frame_pointer();
    let mut pcs = Vec::new();
    walk_fp_chain(start, lo, hi, &mut pcs);
    if TRACES.lock().unwrap().insert(owner, pcs).is_none() {
        TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// Drop the trace owned by `error_ptr`, if any. Called by the GC sweep as it
/// frees each object so a trace lives exactly as long as its error object:
///
///   * without this, `TRACES` grows without bound — every throw leaks an entry
///     forever, even after the error is caught and collected;
///   * and a freed error's address can be handed back by the allocator to a new
///     object, so a stale entry would make the next `throw` at that address hit
///     the capture-once early-out and inherit the dead error's trace.
///
/// Tying removal to the sweep fixes both: the entry goes away the moment the GC
/// proves the error unreachable, before its address can be reused.
pub(crate) fn forget(error_ptr: i64) {
    if error_ptr == 0 || TRACE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if TRACES
        .lock()
        .unwrap()
        .remove(&(error_ptr as usize))
        .is_some()
    {
        TRACE_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Resolve a single captured PC to `(function, file, line)` via the shared
/// symbol table. A PC that lands in no registered Aster function range resolves
/// to `None` and collapses into the `[runtime]` marker.
fn resolve_frame(pc: usize) -> Option<(String, String, i64)> {
    SYMBOLS.lock().unwrap().resolve(pc)
}

/// Allocate a `Frame` object (`function`, `file`: String; `line`: Int) with the
/// layout the lowerer resolves field accesses against. Must run inside a
/// `pause_gc` region because the intermediate object and strings live only in
/// locals, not on the shadow stack.
fn make_frame(function: &str, file: &str, line: i64) -> *mut u8 {
    use super::alloc::aster_class_alloc_typed;
    use super::string::aster_string_new_from_rust;
    use ast::builtin_errors::{FRAME_PTR_COUNT, FRAME_SIZE};

    let obj = aster_class_alloc_typed(FRAME_SIZE, FRAME_PTR_COUNT);
    unsafe {
        let base = obj as *mut i64;
        *base = aster_string_new_from_rust(function) as i64; // function @ 0
        *base.add(1) = aster_string_new_from_rust(file) as i64; // file @ 8
        *base.add(2) = line; // line @ 16
    }
    obj
}

/// Build a `List[Frame]` (a runtime list handle) from captured PCs, collapsing
/// contiguous unresolved runtime frames into a single `[runtime]` marker.
fn build_frame_list(pcs: &[usize]) -> *mut u8 {
    use super::list::{aster_list_new, aster_list_push};

    super::gc::pause_gc(|| {
        let mut handle = aster_list_new(pcs.len().max(1) as i64, 1);
        let mut i = 0;
        while i < pcs.len() {
            match resolve_frame(pcs[i]) {
                Some((function, file, line)) => {
                    let frame = make_frame(&function, &file, line);
                    handle = aster_list_push(handle, frame as i64);
                    i += 1;
                }
                None => {
                    // Collapse the whole contiguous run of unresolved frames.
                    let frame = make_frame("[runtime]", "", 0);
                    handle = aster_list_push(handle, frame as i64);
                    i += 1;
                    while i < pcs.len() && resolve_frame(pcs[i]).is_none() {
                        i += 1;
                    }
                }
            }
        }
        handle
    })
}

/// Runtime entry for `error.trace()`. Returns a `List[Frame]` for the captured
/// trace, ordered worst frame first. Only returns the trace that belongs to
/// this error object (capture-once ownership); otherwise an empty list.
#[unsafe(no_mangle)]
pub extern "C" fn aster_error_trace(error_ptr: i64) -> *mut u8 {
    let pcs = trace_pcs_for(error_ptr);
    build_frame_list(&pcs)
}

/// Captured PCs for `error_ptr`, or empty if it carries no trace.
fn trace_pcs_for(error_ptr: i64) -> Vec<usize> {
    if error_ptr == 0 {
        return Vec::new();
    }
    TRACES
        .lock()
        .unwrap()
        .get(&(error_ptr as usize))
        .cloned()
        .unwrap_or_default()
}

/// Byte layout of the AOT symbol table (all little-endian, offsets relative to
/// the blob start):
///
/// ```text
///   u32 count
///   u32 _pad
///   [count entries, 40 bytes each]:
///     u64 func_addr      (filled by a linker relocation)
///     u32 size
///     u32 def_line
///     u32 name_off, u32 name_len   (into the string region)
///     u32 file_off, u32 file_len
///     u32 lines_off, u32 lines_count  (lines region: `lines_count` (u32,u32) pairs)
///   [string region bytes]
///   [lines region bytes]
/// ```
pub(crate) const AOT_SYM_HEADER: usize = 8;
pub(crate) const AOT_SYM_ENTRY: usize = 40;

#[inline]
unsafe fn read_u32(blob: *const u8, off: usize) -> u32 {
    unsafe { std::ptr::read_unaligned(blob.add(off) as *const u32) }
}
#[inline]
unsafe fn read_u64(blob: *const u8, off: usize) -> u64 {
    unsafe { std::ptr::read_unaligned(blob.add(off) as *const u64) }
}
unsafe fn read_str(blob: *const u8, off: u32, len: u32) -> String {
    if len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(blob.add(off as usize), len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Walk the AOT-emitted symbol data section into the shared symbol table at
/// program init. The function addresses were already resolved by the linker /
/// loader (ordinary relocations), so no dladdr or platform symbol table is used;
/// the binary is self-contained.
#[unsafe(no_mangle)]
pub extern "C" fn aster_register_symbols(blob: *const u8) {
    if blob.is_null() {
        return;
    }
    unsafe {
        let count = read_u32(blob, 0) as usize;
        for i in 0..count {
            let base = AOT_SYM_HEADER + i * AOT_SYM_ENTRY;
            let func_addr = read_u64(blob, base) as usize;
            let size = read_u32(blob, base + 8) as usize;
            let def_line = read_u32(blob, base + 12) as i64;
            let name = read_str(blob, read_u32(blob, base + 16), read_u32(blob, base + 20));
            let file = read_str(blob, read_u32(blob, base + 24), read_u32(blob, base + 28));
            let lines_off = read_u32(blob, base + 32) as usize;
            let lines_count = read_u32(blob, base + 36) as usize;
            let mut lines = Vec::with_capacity(lines_count);
            for j in 0..lines_count {
                let o = lines_off + j * 8;
                lines.push((read_u32(blob, o), read_u32(blob, o + 4)));
            }
            register_function(func_addr, size, name, file, def_line, lines);
        }
    }
}

/// Resolve the trace captured most recently on this thread into
/// `(function, file, line)` tuples for the frames that land in the Aster
/// function table. Test-only: lets end-to-end tests assert real function names
/// and lines after running a program that throws.
#[cfg(test)]
pub(crate) fn current_trace_resolved() -> Vec<(String, String, i64)> {
    let owner = LAST_THROWN.get();
    let pcs = trace_pcs_for(owner as i64);
    pcs.iter().filter_map(|&pc| resolve_frame(pc)).collect()
}

/// Render the trace owned by `error_ptr` as human-readable lines, worst frame
/// first, collapsing contiguous unresolved runtime frames into one `[runtime]`
/// marker. Used by the uncaught-error path at the program entry point.
pub(crate) fn resolved_trace_lines(error_ptr: i64) -> Vec<String> {
    let pcs = trace_pcs_for(error_ptr);
    let mut out = Vec::new();
    let mut i = 0;
    while i < pcs.len() {
        match resolve_frame(pcs[i]) {
            Some((function, file, line)) => {
                out.push(format!("  at {function} ({file}:{line})"));
                i += 1;
            }
            None => {
                out.push("  at [runtime]".to_string());
                i += 1;
                while i < pcs.len() && resolve_frame(pcs[i]).is_none() {
                    i += 1;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Test-harness trace shaping (docs/plans/testing-story.md, task "failure
// location"). These are backend-independent helpers over a resolved trace
// (worst frame / throw site first), plus the minimal harness core that actually
// exercises them: `run_test` runs one `test_*` case and, on a thrown failure,
// `report_test_failure` resolves the captured trace, takes the failing test's
// call-site as the injected assertion span (`#[track_caller]`-style: the throw
// surfaces at the caller's line in the test), trims the harness frames below the
// `test_*` definition, and pairs the two into a `FailureSite`.
//
// This is the stack-trace side of the story: the trim-and-pair mechanism, wired
// into a real run of a compiled `test_*` function. The FULL `asterc test`
// harness — file/`test_*` discovery, the `std/test` assertion library and
// `AssertionError` type, the seeded runner, and the formatter seam — is the
// separate testing-story feature (issue #2) and builds on this core.
// ---------------------------------------------------------------------------

/// One resolved trace frame: Aster function name, source file, 1-based line.
pub type ResolvedFrame = (String, String, i64);

/// Does `function` name a test entry point? Unit tests are top-level
/// `def test_*` functions (the convention core in docs/plans/testing-story.md);
/// the `test_` prefix is the discovery rule and the trim boundary.
pub fn is_test_frame(function: &str) -> bool {
    function.starts_with("test_")
}

/// Trim the harness frames below the failing `test_*` definition. A captured
/// trace is worst-first: `[throw_site, .., test_fn, harness_loop, runtime..]`.
/// Everything from the throw site down to and including the first `test_*` frame
/// is the user-relevant failure path; every frame below it is the harness that
/// invoked the test and is noise. Returns the trace truncated after the first
/// `test_*` frame, or unchanged when the trace contains no `test_*` frame.
pub fn trim_below_test_frame(frames: &[ResolvedFrame]) -> Vec<ResolvedFrame> {
    match frames.iter().position(|(f, _, _)| is_test_frame(f)) {
        Some(idx) => frames[..=idx].to_vec(),
        None => frames.to_vec(),
    }
}

/// A test failure's trace paired with the assertion call-site the harness
/// injects (`#[track_caller]`-style): the `(file, line)` of the `assert*` call
/// that threw, plus the frames trimmed to the failing `test_*` definition. This
/// is the shape a harness failure event carries (docs/plans/testing-story.md:
/// "the event format reserves a trace field either way").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureSite {
    pub assertion_file: String,
    pub assertion_line: i64,
    pub frames: Vec<ResolvedFrame>,
}

/// Pair a captured trace with the injected assertion call-site span, trimming
/// the harness frames below the failing `test_*` definition.
pub fn pair_failure_with_assertion(
    frames: &[ResolvedFrame],
    assertion_file: &str,
    assertion_line: i64,
) -> FailureSite {
    FailureSite {
        assertion_file: assertion_file.to_string(),
        assertion_line,
        frames: trim_below_test_frame(frames),
    }
}

/// A harness-level test failure: the failing test's name, the injected
/// assertion call-site, and the trace trimmed to the failing `test_*`
/// definition. This is the shape a harness failure event carries into a
/// formatter (docs/plans/testing-story.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestFailure {
    pub test_name: String,
    pub site: FailureSite,
}

impl TestFailure {
    /// Render the failure the way a harness formatter would: the failing test
    /// and its assertion call-site, then the trimmed trace worst frame first.
    pub fn render(&self) -> Vec<String> {
        let mut out = vec![format!(
            "FAIL {} ({}:{})",
            self.test_name, self.site.assertion_file, self.site.assertion_line
        )];
        for (function, file, line) in &self.site.frames {
            out.push(format!("  at {function} ({file}:{line})"));
        }
        out
    }
}

/// Resolve the captured trace owned by `error_ptr` into `(function, file, line)`
/// tuples for the frames that land in the Aster function table, worst frame
/// (throw site) first. Runtime-internal frames that resolve to nothing are
/// dropped — the harness reasons about Aster frames only.
pub fn resolve_trace(error_ptr: i64) -> Vec<ResolvedFrame> {
    trace_pcs_for(error_ptr)
        .iter()
        .filter_map(|&pc| resolve_frame(pc))
        .collect()
}

/// Turn a failed `test_*` run into a harness failure report — the wiring the
/// acceptance criterion names. Given the error a `test_*` function threw:
/// resolve its captured trace, take the failing test's frame as the injected
/// assertion call-site (`#[track_caller]`-style: an assertion helper's throw
/// surfaces at the line in the test that invoked it — the return address on the
/// test's frame), trim the harness frames below the `test_*` definition, and
/// pair the two. Returns `None` when the error carries no trace or the trace
/// has no `test_*` frame (nothing a test harness would report).
pub fn report_test_failure(test_name: &str, error_ptr: i64) -> Option<TestFailure> {
    let frames = resolve_trace(error_ptr);
    let test_idx = frames.iter().position(|(f, _, _)| is_test_frame(f))?;
    let (_, assertion_file, assertion_line) = frames[test_idx].clone();
    let site = pair_failure_with_assertion(&frames, &assertion_file, assertion_line);
    Some(TestFailure {
        test_name: test_name.to_string(),
        site,
    })
}

/// Run one discovered `test_*` case and classify it. `run` invokes the compiled
/// test body; a clean return is a pass (`None`), a thrown error is a failure
/// whose captured trace is trimmed to the `test_*` definition and paired with
/// the injected assertion call-site (see `report_test_failure`).
///
/// This is the minimal harness core the stack-trace work owes the testing story
/// (issue #2): test discovery, the assertion library, seeded ordering, and the
/// formatter all live there and drive cases through this function.
pub fn run_test(test_name: &str, run: impl FnOnce()) -> Option<TestFailure> {
    use super::error::{error_flag_get, error_flag_set, error_value_get};
    // Enter each case with a clear error slot so a prior case can't be misread
    // as this one failing.
    error_flag_set(false);
    run();
    if !error_flag_get() {
        return None; // clean return -> PASS
    }
    let error_ptr = error_value_get();
    // Consume the flag: the harness has classified this case, it does not
    // propagate past the runner.
    error_flag_set(false);
    report_test_failure(test_name, error_ptr)
}

/// Record the calling (main) thread's stack bounds at program start. Emitted
/// into both the AOT `main` wrapper and the JIT run path so the very first
/// throw from `main` walks within recorded bounds.
#[unsafe(no_mangle)]
pub extern "C" fn aster_runtime_init() {
    record_os_thread_bounds();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_thread_bounds_contain_a_local() {
        let (lo, hi) = os_thread_bounds().expect("os stack bounds available on linux/macos");
        assert!(lo > 0 && hi > lo, "bounds must be a non-empty range");
        let local = 0u64;
        let addr = &local as *const u64 as usize;
        assert!(
            addr >= lo && addr < hi,
            "a stack local {addr:#x} must lie within [{lo:#x}, {hi:#x})"
        );
    }

    #[test]
    fn test_walk_with_unknown_bounds_is_empty_and_safe() {
        // Degenerate bounds must disable walking entirely: no dereference, no
        // crash, empty result. This is the fail-safe for contexts whose bounds
        // were never recorded (e.g. a throw from a green thread that never had
        // its mmap'd bounds set).
        let mut out = vec![1usize, 2, 3];
        walk_fp_chain(current_frame_pointer(), 0, 0, &mut out);
        assert!(out.is_empty(), "unknown bounds must produce an empty trace");
    }

    #[test]
    fn test_walk_never_reads_outside_bounds() {
        // A window that excludes the real frame pointer must yield nothing
        // rather than dereferencing out-of-range memory.
        let fp = current_frame_pointer();
        let mut out = Vec::new();
        // Bounds entirely below fp: fp < lo, so the first check bails.
        walk_fp_chain(fp, 1, 4096, &mut out);
        assert!(out.is_empty());
    }

    fn seed_trace(owner: usize, pcs: Vec<usize>) {
        if TRACES.lock().unwrap().insert(owner, pcs).is_none() {
            TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn stored_trace(owner: usize) -> Vec<usize> {
        TRACES
            .lock()
            .unwrap()
            .get(&owner)
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn test_capture_walk_is_bounded_and_nonempty() {
        record_os_thread_bounds();
        // A distinctive fake key that will not collide with a real heap pointer.
        let key = 0x1234_0001;
        TRACES.lock().unwrap().remove(&key);
        capture_for(key as i64);
        let len = stored_trace(key).len();
        assert!(len >= 1, "a real throw frame chain must capture >= 1 frame");
        assert!(
            len <= MAX_TRACE_FRAMES,
            "capture must respect the frame cap"
        );
    }

    #[test]
    fn test_capture_once_same_owner_preserves_trace() {
        // Seed a known trace owned by a distinctive fake key.
        let key = 0xAAAA_0001;
        seed_trace(key, vec![10, 20, 30]);
        // A throw of the same error value must NOT recapture.
        capture_for(key as i64);
        assert_eq!(
            stored_trace(key),
            vec![10, 20, 30],
            "same-owner throw preserves the trace"
        );
    }

    #[test]
    fn test_new_owner_captures_fresh_trace() {
        record_os_thread_bounds();
        let old = 0xAAAA_0002;
        let new = 0xBBBB_0002;
        seed_trace(old, vec![10, 20, 30]);
        TRACES.lock().unwrap().remove(&new);
        // A different error value captures fresh under its own key.
        capture_for(new as i64);
        assert_ne!(stored_trace(new), vec![10, 20, 30], "new owner recaptures");
        assert!(!stored_trace(new).is_empty(), "new owner has its own trace");
    }

    #[test]
    fn test_build_frame_list_collapses_unresolved_into_single_runtime_marker() {
        // With no symbol table, several PCs collapse to one [runtime] frame.
        let handle = build_frame_list(&[0xdead, 0xbeef, 0xcafe]);
        let len = super::super::list::aster_list_len(handle);
        assert_eq!(
            len, 1,
            "contiguous unresolved frames collapse to one marker"
        );
    }

    #[test]
    fn test_build_frame_list_empty_pcs_is_empty_list() {
        let handle = build_frame_list(&[]);
        let len = super::super::list::aster_list_len(handle);
        assert_eq!(len, 0);
    }

    fn sym(
        start: usize,
        len: usize,
        name: &str,
        file: &str,
        line: i64,
        lines: Vec<(u32, u32)>,
    ) -> FuncSym {
        FuncSym {
            start,
            len,
            name: name.into(),
            file: file.into(),
            def_line: line,
            lines,
        }
    }

    #[test]
    fn test_resolve_returns_real_frame_for_pc_in_range() {
        let mut tbl =
            SymTable::from_entries(vec![sym(0x1000, 0x100, "deep", "prog.aster", 7, vec![])]);
        let (name, file, line) = tbl.resolve(0x1042).expect("pc inside a range resolves");
        assert_eq!(name, "deep");
        assert_eq!(file, "prog.aster");
        assert_eq!(line, 7, "no line table falls back to the definition line");
    }

    #[test]
    fn test_resolve_pc_outside_all_ranges_is_none() {
        let mut tbl =
            SymTable::from_entries(vec![sym(0x1000, 0x100, "deep", "prog.aster", 7, vec![])]);
        assert!(
            tbl.resolve(0x2000).is_none(),
            "a PC past every range must not resolve (collapses to [runtime])"
        );
        assert!(
            tbl.resolve(0x0fff).is_none(),
            "a PC below every range is unresolved"
        );
    }

    #[test]
    fn test_line_table_maps_pc_offset_to_line() {
        // A function spanning source lines 10..12: offset 0 -> line 10,
        // offset 0x20 -> line 11, offset 0x40 -> line 12.
        let mut tbl = SymTable::from_entries(vec![sym(
            0x4000,
            0x80,
            "middle",
            "prog.aster",
            10,
            vec![(0, 10), (0x20, 11), (0x40, 12)],
        )]);
        // A return address at start+0x26 (-1 step-back) lands in the
        // [0x20, 0x40) region -> line 11.
        let (_, _, line) = tbl.resolve(0x4000 + 0x26).expect("resolves");
        assert_eq!(
            line, 11,
            "line table resolves the PC offset to its source line"
        );
    }

    fn frame(name: &str, line: i64) -> ResolvedFrame {
        (name.into(), "spec_test.aster".into(), line)
    }

    #[test]
    fn test_trim_drops_harness_frames_below_the_test_definition() {
        // Worst-first: throw site, the assertion helper, the failing test_*
        // def, then the harness loop + runtime that invoked it. Trimming keeps
        // up to and including `test_adds`, dropping `__test_harness` and below.
        let trace = vec![
            frame("assert_eq", 12),
            frame("test_adds", 30),
            frame("__test_harness", 4),
            frame("[runtime]", 0),
        ];
        let trimmed = trim_below_test_frame(&trace);
        assert_eq!(
            trimmed,
            vec![frame("assert_eq", 12), frame("test_adds", 30)],
            "everything below the test_* frame is trimmed"
        );
    }

    #[test]
    fn test_trim_keeps_whole_trace_when_no_test_frame_present() {
        let trace = vec![frame("deep", 5), frame("main", 9)];
        assert_eq!(
            trim_below_test_frame(&trace),
            trace,
            "a trace with no test_* frame is returned unchanged"
        );
    }

    #[test]
    fn test_trim_stops_at_the_first_test_frame() {
        // Only the shallowest test_* boundary matters; a nested test_* helper
        // deeper up must not extend the trim past the first test frame.
        let trace = vec![
            frame("test_helper", 3),
            frame("test_outer", 20),
            frame("__test_harness", 1),
        ];
        assert_eq!(
            trim_below_test_frame(&trace),
            vec![frame("test_helper", 3)],
            "trim boundary is the first (worst-first) test_* frame"
        );
    }

    #[test]
    fn test_is_test_frame_matches_only_the_convention_prefix() {
        assert!(is_test_frame("test_adds"));
        assert!(is_test_frame("test_"));
        assert!(!is_test_frame("testing"));
        assert!(!is_test_frame("my_test_"));
        assert!(!is_test_frame("main"));
    }

    #[test]
    fn test_pair_failure_carries_assertion_site_and_trimmed_frames() {
        let trace = vec![
            frame("assert", 8),
            frame("test_math", 40),
            frame("__test_harness", 2),
        ];
        let site = pair_failure_with_assertion(&trace, "math_test.aster", 8);
        assert_eq!(site.assertion_file, "math_test.aster");
        assert_eq!(site.assertion_line, 8);
        assert_eq!(
            site.frames,
            vec![frame("assert", 8), frame("test_math", 40)],
            "the paired trace is trimmed to the failing test_* definition"
        );
    }

    #[test]
    fn test_forget_removes_the_trace_so_a_reused_address_recaptures_fresh() {
        // Model the GC lifecycle: an error is thrown (its address gets a trace),
        // then collected (`forget`), then its address is handed to a NEW error
        // that throws. The new throw must capture its own trace, not inherit the
        // dead error's — the bug capture-once would hit if the entry lingered.
        record_os_thread_bounds();
        let addr = 0xCCCC_0004;
        seed_trace(addr, vec![111, 222, 333]);
        forget(addr as i64);
        assert!(
            stored_trace(addr).is_empty(),
            "forget drops the collected error's trace"
        );
        capture_for(addr as i64);
        assert_ne!(
            stored_trace(addr),
            vec![111, 222, 333],
            "a reused address recaptures fresh rather than inheriting the dead trace"
        );
        forget(addr as i64);
    }

    #[test]
    fn test_forget_is_a_noop_for_an_untraced_pointer() {
        // Freeing a non-error object (no trace) must not disturb other entries.
        let live = 0xDDDD_0004;
        seed_trace(live, vec![7, 8, 9]);
        forget(0xEEEE_0004); // never traced
        assert_eq!(
            stored_trace(live),
            vec![7, 8, 9],
            "unrelated entry survives"
        );
        forget(live as i64);
    }

    #[test]
    fn test_aster_error_trace_returns_empty_for_unowned_error() {
        seed_trace(0x1111_0003, vec![1, 2, 3]);
        // A different (or null) error pointer must not leak another's trace.
        let handle = aster_error_trace(0x2222_0003);
        let len = super::super::list::aster_list_len(handle);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_report_test_failure_trims_and_pairs_from_a_resolved_trace() {
        // Register fake function ranges so seeded PCs resolve to named frames,
        // then seed a trace shaped exactly like a real failure, worst-first:
        //   assert_eq (throw site) -> test_case -> __harness (below the test).
        // High, isolated addresses so these never collide with real JIT/AOT code.
        register_function(
            0x5000_0000,
            0x100,
            "assert_eq".into(),
            "m_test.aster".into(),
            3,
            vec![],
        );
        register_function(
            0x5000_1000,
            0x100,
            "test_case".into(),
            "m_test.aster".into(),
            20,
            vec![],
        );
        register_function(
            0x5000_2000,
            0x100,
            "__harness".into(),
            "m_test.aster".into(),
            1,
            vec![],
        );
        let key = 0x7777_5001;
        seed_trace(key, vec![0x5000_0040, 0x5000_1040, 0x5000_2040]);

        let failure =
            report_test_failure("test_case", key as i64).expect("a test_* frame yields a report");
        assert_eq!(failure.test_name, "test_case");
        // The injected assertion call-site is the failing test frame's file:line
        // (where the assertion was invoked), not the helper's internal throw line.
        assert_eq!(failure.site.assertion_file, "m_test.aster");
        assert_eq!(failure.site.assertion_line, 20);
        let names: Vec<&str> = failure
            .site
            .frames
            .iter()
            .map(|(f, _, _)| f.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["assert_eq", "test_case"],
            "trace trimmed at the test_* boundary; the __harness frame below it is dropped"
        );
        forget(key as i64);
    }

    #[test]
    fn test_report_test_failure_is_none_without_a_test_frame() {
        register_function(
            0x5100_0000,
            0x100,
            "helper".into(),
            "x.aster".into(),
            2,
            vec![],
        );
        let key = 0x7777_5002;
        seed_trace(key, vec![0x5100_0040]);
        assert!(
            report_test_failure("nope", key as i64).is_none(),
            "a trace with no test_* frame is not a harness-reportable failure"
        );
        forget(key as i64);
    }

    #[test]
    fn test_test_failure_render_lists_site_then_trimmed_frames() {
        let site = FailureSite {
            assertion_file: "m_test.aster".into(),
            assertion_line: 20,
            frames: vec![frame("assert_eq", 3), frame("test_case", 20)],
        };
        let out = TestFailure {
            test_name: "test_case".into(),
            site,
        }
        .render();
        assert_eq!(out[0], "FAIL test_case (m_test.aster:20)");
        assert_eq!(out[1], "  at assert_eq (spec_test.aster:3)");
        assert_eq!(out[2], "  at test_case (spec_test.aster:20)");
    }
}
