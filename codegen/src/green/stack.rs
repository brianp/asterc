use std::sync::Mutex;

const DEFAULT_STACK_SIZE: usize = 64 * 1024; // 64 KB usable
const DEFAULT_GUARD_SIZE: usize = 4096; // 4 KB guard page

pub(crate) struct GreenStack {
    base: *mut u8,
    total: usize, // guard + usable
}

unsafe impl Send for GreenStack {}

impl GreenStack {
    pub(crate) fn alloc(size: usize, guard: usize) -> Self {
        let total = size + guard;
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            ) as *mut u8
        };
        assert!(
            base != libc::MAP_FAILED as *mut u8,
            "mmap failed for green stack"
        );
        // Guard page at the bottom — stack overflow hits PROT_NONE → SIGSEGV
        let rc = unsafe { libc::mprotect(base as *mut _, guard, libc::PROT_NONE) };
        assert!(rc == 0, "mprotect failed for guard page");
        Self { base, total }
    }

    /// Top of the usable stack region (stack grows down).
    pub(crate) fn top(&self) -> *mut u8 {
        unsafe { self.base.add(self.total) }
    }
}

impl Drop for GreenStack {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base as *mut _, self.total);
        }
    }
}

pub(crate) struct StackPool {
    free: Mutex<Vec<GreenStack>>,
    stack_size: usize,
    guard_size: usize,
    max_cached: usize,
}

impl StackPool {
    pub(crate) fn new(max_cached: usize) -> Self {
        Self {
            free: Mutex::new(Vec::new()),
            stack_size: DEFAULT_STACK_SIZE,
            guard_size: DEFAULT_GUARD_SIZE,
            max_cached,
        }
    }

    pub(crate) fn get(&self) -> GreenStack {
        if let Some(stack) = self.free.lock().unwrap().pop() {
            return stack;
        }
        GreenStack::alloc(self.stack_size, self.guard_size)
    }

    pub(crate) fn put(&self, stack: GreenStack) {
        let mut free = self.free.lock().unwrap();
        if free.len() < self.max_cached {
            free.push(stack);
        }
        // else: dropped here → munmap
    }
}
