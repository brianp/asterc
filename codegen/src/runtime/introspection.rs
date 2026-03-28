use super::list::aster_list_new;
use super::string::aster_string_new;

/// Helper: create a heap-allocated string from a Rust &str.
fn make_string(s: &str) -> *mut u8 {
    aster_string_new(s.as_ptr(), s.len())
}

/// Helper: read a Rust string from an Aster string pointer.
/// Safety: the pointer must be a valid Aster string (len at offset 0, data at offset 8).
unsafe fn read_string(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let len = *(ptr as *const i64);
        if len <= 0 {
            return "";
        }
        let data = ptr.add(8);
        let slice = std::slice::from_raw_parts(data, len as usize);
        std::str::from_utf8_unchecked(slice)
    }
}

/// Return the class name as a Type value (which is a string pointer at runtime).
/// The argument is a string literal with the static type name.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_class_name(type_name: *const u8) -> *mut u8 {
    // Type values are represented as strings at runtime.
    // We return a copy of the type name string.
    let name = unsafe { read_string(type_name) };
    make_string(name)
}

/// Return the fields list for a type. Returns List[FieldInfo].
/// For the initial implementation, returns an empty list.
/// Full implementation requires compile-time metadata embedding.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_fields(_type_name: *const u8) -> *mut u8 {
    // Return an empty list (pointer-type elements since FieldInfo is a class)
    aster_list_new(0, 1)
}

/// Return the methods list for a type. Returns List[MethodInfo].
/// For the initial implementation, returns an empty list.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_methods(_type_name: *const u8) -> *mut u8 {
    aster_list_new(0, 1)
}

/// Return the ancestors list for a type. Returns List[Type].
/// For the initial implementation, returns a list containing just the type itself.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_ancestors(type_name: *const u8) -> *mut u8 {
    let list = aster_list_new(1, 1);
    let name_copy = aster_introspect_class_name(type_name);
    super::list::aster_list_push(list, name_copy as i64);
    list
}

/// Return the children list for a type. Returns List[Type].
/// For the initial implementation, returns an empty list.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_children(_type_name: *const u8) -> *mut u8 {
    aster_list_new(0, 1)
}

/// Check if an instance's type is a subtype of the target type.
/// Both arguments are string pointers with type names.
/// Returns 1 (true) if the type name matches, 0 (false) otherwise.
/// Note: in the initial implementation, this only checks exact string equality
/// since the full class hierarchy is not yet embedded in the binary.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_is_a(type_name: *const u8, target_name: *const u8) -> u8 {
    let ty = unsafe { read_string(type_name) };
    let target = unsafe { read_string(target_name) };
    if ty == target { 1 } else { 0 }
}

/// Check if a type responds to a method name.
/// First argument is the type name string, second is the method name string.
/// For the initial implementation, returns 0 (false) since method metadata
/// is not yet embedded in the binary.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_responds_to(
    _type_name: *const u8,
    _method_name: *const u8,
) -> u8 {
    0
}
