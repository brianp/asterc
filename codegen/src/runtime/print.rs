/// Say a string (ptr to heap string object).
/// String layout: [len: i64][data: u8...]
#[unsafe(no_mangle)]
pub extern "C" fn aster_say_str(ptr: *const u8) {
    if ptr.is_null() {
        println!("nil");
        return;
    }
    const MAX_STRING_LENGTH: usize = 1_000_000;
    unsafe {
        let raw_len = *(ptr as *const i64);
        if raw_len < 0 || raw_len as usize > MAX_STRING_LENGTH {
            println!("<invalid string: length {} out of bounds>", raw_len);
            return;
        }
        let len = raw_len as usize;
        let data = ptr.add(8);
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("{}", String::from_utf8_lossy(bytes)),
        }
    }
}

/// Say an integer.
#[unsafe(no_mangle)]
pub extern "C" fn aster_say_int(val: i64) {
    println!("{}", val);
}

/// Say a float.
#[unsafe(no_mangle)]
pub extern "C" fn aster_say_float(val: f64) {
    println!("{}", val);
}

/// Say a bool.
#[unsafe(no_mangle)]
pub extern "C" fn aster_say_bool(val: i8) {
    println!("{}", if val != 0 { "true" } else { "false" });
}
