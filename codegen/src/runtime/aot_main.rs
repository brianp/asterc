//! AOT binary entry point.
//!
//! When linking an AOT binary, this provides the `main` function that
//! calls into the user's `aster_main` (emitted by the AOT codegen).

unsafe extern "C" {
    fn aster_main() -> i64;
}

#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let result = unsafe { aster_main() };
    result as i32
}
