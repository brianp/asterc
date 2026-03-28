use super::alloc::aster_class_alloc_typed;
use super::list::{aster_list_new, aster_list_push};
use super::string::aster_string_new;

/// Helper: create a heap-allocated Aster string from a Rust &str.
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

/// Construct a FieldInfo instance on the heap.
/// Layout (ptr fields first): name(ptr,0), type_name(ptr,8), is_public(val,16)
fn make_field_info(name: &str, type_name: &str, is_public: bool) -> *mut u8 {
    let obj = aster_class_alloc_typed(24, 2);
    let name_ptr = make_string(name);
    let type_ptr = make_string(type_name);
    unsafe {
        *(obj as *mut i64) = name_ptr as i64; // offset 0: name
        *((obj as *mut i64).add(1)) = type_ptr as i64; // offset 8: type_name
        *((obj as *mut i64).add(2)) = i64::from(is_public); // offset 16: is_public
    }
    obj
}

/// Construct a ParamInfo instance on the heap.
/// Layout (ptr fields first): name(ptr,0), param_type(ptr,8), has_default(val,16)
fn make_param_info(name: &str, param_type: &str, has_default: bool) -> *mut u8 {
    let obj = aster_class_alloc_typed(24, 2);
    let name_ptr = make_string(name);
    let type_ptr = make_string(param_type);
    unsafe {
        *(obj as *mut i64) = name_ptr as i64;
        *((obj as *mut i64).add(1)) = type_ptr as i64;
        *((obj as *mut i64).add(2)) = i64::from(has_default);
    }
    obj
}

/// Construct a MethodInfo instance on the heap.
/// Layout (ptr fields first): name(ptr,0), params(ptr,8), return_type(ptr,16), is_public(val,24)
fn make_method_info(
    name: &str,
    params_list: *mut u8,
    return_type: &str,
    is_public: bool,
) -> *mut u8 {
    let obj = aster_class_alloc_typed(32, 3);
    let name_ptr = make_string(name);
    let ret_ptr = make_string(return_type);
    unsafe {
        *(obj as *mut i64) = name_ptr as i64; // offset 0: name
        *((obj as *mut i64).add(1)) = params_list as i64; // offset 8: params
        *((obj as *mut i64).add(2)) = ret_ptr as i64; // offset 16: return_type
        *((obj as *mut i64).add(3)) = i64::from(is_public); // offset 24: is_public
    }
    obj
}

/// Return the class name as a Type value (which is a string pointer at runtime).
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_class_name(type_name: *const u8) -> *mut u8 {
    let name = unsafe { read_string(type_name) };
    make_string(name)
}

/// Return the fields list for a type. Returns List[FieldInfo].
/// Argument is a serialized string: "name:TypeName:1|name2:TypeName2:0"
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_fields(serialized: *const u8) -> *mut u8 {
    let data = unsafe { read_string(serialized) };

    if data.is_empty() {
        return aster_list_new(0, 1);
    }

    let entries: Vec<&str> = data.split('|').collect();
    let list = aster_list_new(entries.len() as i64, 1);

    for entry in entries {
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() >= 3 {
            let name = parts[0];
            let type_name = parts[1];
            let is_public = parts[2] == "1";
            let fi = make_field_info(name, type_name, is_public);
            aster_list_push(list, fi as i64);
        }
    }

    list
}

/// Return the methods list for a type. Returns List[MethodInfo].
/// Argument is serialized: "name:RetType:is_pub:p1/T1,p2/T2|name2:..."
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_methods(serialized: *const u8) -> *mut u8 {
    let data = unsafe { read_string(serialized) };

    if data.is_empty() {
        return aster_list_new(0, 1);
    }

    let entries: Vec<&str> = data.split('|').collect();
    let list = aster_list_new(entries.len() as i64, 1);

    for entry in entries {
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() >= 4 {
            let name = parts[0];
            let ret_type = parts[1];
            let is_public = parts[2] == "1";
            let params_str = parts[3];

            // Build params list
            let params_list = if params_str.is_empty() {
                aster_list_new(0, 1)
            } else {
                let param_entries: Vec<&str> = params_str.split(',').collect();
                let plist = aster_list_new(param_entries.len() as i64, 1);
                for param in param_entries {
                    if let Some((pname, ptype)) = param.split_once('/') {
                        let pi = make_param_info(pname, ptype, false);
                        aster_list_push(plist, pi as i64);
                    }
                }
                plist
            };

            let mi = make_method_info(name, params_list, ret_type, is_public);
            aster_list_push(list, mi as i64);
        }
    }

    list
}

/// Return the ancestors list for a type. Returns List[Type].
/// Argument is serialized: "Type1|Type2|Type3" (self first, root last).
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_ancestors(serialized: *const u8) -> *mut u8 {
    let data = unsafe { read_string(serialized) };
    build_type_list(data)
}

/// Return the children list for a type. Returns List[Type].
/// Argument is serialized: "Child1|Child2" or empty string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_children(serialized: *const u8) -> *mut u8 {
    let data = unsafe { read_string(serialized) };
    build_type_list(data)
}

/// Build a List[Type] from a pipe-delimited string of type names.
fn build_type_list(data: &str) -> *mut u8 {
    if data.is_empty() {
        return aster_list_new(0, 1);
    }

    let names: Vec<&str> = data.split('|').collect();
    let list = aster_list_new(names.len() as i64, 1);
    for name in names {
        let type_val = make_string(name);
        aster_list_push(list, type_val as i64);
    }
    list
}

/// Check if a type responds to a method name.
/// First argument is a pipe-delimited member list, second is the method name string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_responds_to(
    member_list: *const u8,
    method_name: *const u8,
) -> u8 {
    let members = unsafe { read_string(member_list) };
    let target = unsafe { read_string(method_name) };

    if members.is_empty() || target.is_empty() {
        return 0;
    }

    for member in members.split('|') {
        if member == target {
            return 1;
        }
    }
    0
}

/// Check if an instance's type is a subtype of the target type.
/// This is no longer called (resolved at compile time), but kept for safety.
#[unsafe(no_mangle)]
pub extern "C" fn aster_introspect_is_a(_type_name: *const u8, _target_name: *const u8) -> u8 {
    // Compile-time resolved; this should never be called.
    0
}
