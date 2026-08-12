# RFC: Transparent BigInt Promotion

Status: DRAFT

> **Interim measure (2026-03-25):** Overflow-checked arithmetic is now in place.
> `aster_int_add`, `aster_int_sub`, and `aster_int_mul` runtime functions abort
> on overflow instead of silently wrapping. When BigInt is implemented, these
> functions should be replaced with the promotion-based `aster_int_*` functions
> described in Section 5 of this RFC. The codegen in `translate_binop` already
> routes integer add/sub/mul through runtime calls, so the cutover will only
> require updating the runtime functions (no codegen changes needed).

---

## 1. Motivation

Aster's `Int` type is a 64-bit signed integer. Arithmetic that exceeds this range silently wraps (C-style overflow), producing incorrect results:

```
say(message: "{2**63}")   # prints 0 (wrong — should be 9223372036854775808)
say(message: "{2**100}")  # prints 0 (wrong — should be 1267650600228229401496703205376)
say(message: "{100**100}")  # garbage (wrong)
```

This is a correctness problem. Integer arithmetic should produce correct results regardless of magnitude. Languages like Ruby and Python handle this transparently — the user never thinks about integer size.

---

## 2. Design Principles

1. **Transparent** — the user sees one type: `Int`. There is no `BigInt` type in the language. Promotion and demotion happen automatically.
2. **Correct** — arithmetic never silently overflows. `2**100` returns the mathematically correct value.
3. **Fast for small values** — the common case (numbers that fit in 63 bits) should have near-zero overhead.
4. **Compatible** — existing code continues to work. `Int` is still `Int`. Comparison, hashing, printing all work on promoted values.

---

## 3. Representation: Tagged Integers

All integer values use a tagged representation in a single 64-bit word:

```
Small int (fits in 63 bits):
  [63-bit signed value][1]    ← low bit = 1 means "small int"
  Actual value = word >> 1 (arithmetic shift)
  Range: -4,611,686,018,427,387,904 to 4,611,686,018,427,387,903 (±2^62)

Big int (exceeds 63 bits):
  [pointer to heap BigInt][0]  ← low bit = 0 means "heap pointer"
  Pointer is always aligned to 8+ bytes, so low bits are 0 naturally.
```

### 3.1 Small Int Range

Small ints cover ±2^62, approximately ±4.6 × 10^18. For reference:
- Nanoseconds since the Big Bang: ~4.3 × 10^26 (needs BigInt)
- Seconds since the Big Bang: ~4.3 × 10^17 (fits in small int)
- US national debt in cents: ~3.6 × 10^15 (fits easily)
- Maximum array index anyone would use: fits easily
- `2**62`: 4,611,686,018,427,387,904 (just at the boundary)
- `2**63`: promoted to BigInt

This is the same approach Ruby uses. The 1-bit cost is negligible for real-world programs.

### 3.2 Heap BigInt Layout

A BigInt on the heap is a GC-managed object with the following layout:

```
GC Header (16 bytes): [mark][type=OBJ_BIGINT][magic][size][next]
Payload:
  [sign: i8][len: i32][padding: 3 bytes][digits: u64 × len]
```

- `sign`: 0 = positive, 1 = negative
- `len`: number of 64-bit limbs
- `digits`: little-endian array of unsigned 64-bit limbs

Alternative: use a proven library like mini-gmp or craft a minimal big integer implementation in the runtime. The exact limb representation is an implementation detail.

### 3.3 Why Not NaN-Boxing or Other Schemes?

Tagged low-bit is the simplest scheme that works. NaN-boxing is better for dynamically typed languages where every value could be int/float/pointer. Aster is statically typed — we know at compile time whether a value is `Int`, `Float`, `Bool`, or `Ptr`. Only `Int` needs the tag.

`Float` and `Bool` remain untagged — `Float` is a raw `f64`, `Bool` is an `i8`. No change.

---

## 4. Arithmetic Operations

Every arithmetic operation on `Int` follows the same pattern:

```
fn add(a: tagged_int, b: tagged_int) -> tagged_int:
  if both_small(a, b):
    result = small_add(a, b)  # raw 64-bit add on shifted values
    if no_overflow:
      return tag_small(result)
    else:
      promote both to BigInt, add, return tagged pointer
  else:
    promote whichever is small to BigInt
    big_add(a_big, b_big)
    try_demote_to_small(result)  # if result fits in 63 bits, return small
```

### 4.1 Operations That Need Promotion Logic

| Operation | Overflow possible? | Notes |
|-----------|-------------------|-------|
| `+`, `-` | Yes | Checked add/sub on small ints |
| `*` | Yes | Widening multiply, check overflow |
| `**` | Yes | Most common source of BigInts |
| `/` | No* | Truncating division; result ≤ inputs. *Exception: `MIN / -1` |
| `%` | No | Result ≤ divisor |
| Unary `-` | Rare | Only overflows for `-MIN` |

### 4.2 Comparison

Comparison must handle mixed small/big cases:

```
small vs small:  direct integer comparison (fast path)
small vs big:    promote small to big, compare
big vs big:      compare sign, then limbs
```

`Eq` and `Ord` protocol implementations on `Int` automatically handle both representations. User-defined `Eq`/`Ord` on classes with `Int` fields work unchanged.

### 4.3 Division Semantics

Integer division truncates toward zero, same as today:

```
10 / 3      # 3
-10 / 3     # -3
big / big   # truncated BigInt
```

If you want a float result, use float operands: `10.0 / 3.0`.

### 4.4 Float Interaction

`Int + Float` promotes the `Int` to `Float`, same as today. For BigInts that exceed `f64` precision (~2^53), this loses precision silently — matching Ruby's behavior. This is the only case where precision is lost, and it's explicit: the user chose to mix types.

```
let x = 2**100         # BigInt
let y = x + 1.0        # Float (loses precision, like Ruby)
let z = x + 1          # BigInt (exact)
```

---

## 5. Runtime Functions

### 5.1 New Runtime Functions

```
aster_int_add(a: i64, b: i64) -> i64       # checked small add, promotes on overflow
aster_int_sub(a: i64, b: i64) -> i64       # checked small sub
aster_int_mul(a: i64, b: i64) -> i64       # checked small mul
aster_int_div(a: i64, b: i64) -> i64       # handles MIN/-1 edge case
aster_int_mod(a: i64, b: i64) -> i64       # modulo
aster_int_neg(a: i64) -> i64               # unary negation
aster_int_pow(a: i64, b: i64) -> i64       # power (most likely to overflow)
aster_int_cmp(a: i64, b: i64) -> i64       # comparison (-1, 0, 1)
aster_int_eq(a: i64, b: i64) -> i8         # equality
aster_int_to_f64(a: i64) -> f64            # convert to float
aster_int_to_string(a: i64) -> *mut u8     # updated to handle both representations
aster_bigint_new(sign: i8, limbs: ...) -> i64  # create a BigInt, return tagged pointer
```

### 5.2 Updated Runtime Functions

These existing functions need to understand tagged ints:

- `aster_say_int` — must detect tag and print correctly
- `aster_int_to_string` — must handle BigInt representation
- `aster_pow_int` → replaced by `aster_int_pow` with promotion
- All list/map operations that store `i64` values — no change needed (tagged values are still i64-sized)

### 5.3 GC Integration

BigInt heap objects get a new GC type tag `OBJ_BIGINT`. During mark phase:
- `is_gc_payload` must handle tagged integers: if low bit is 1, it's a small int (not a pointer). If low bit is 0 and it's in heap range, check for GC header.
- List elements that are tagged integers with low bit 0 (BigInt pointers) ARE traceable by the GC. List elements with low bit 1 (small ints) are not.
- This replaces the current `OBJ_LIST_HANDLE_NOPTR` approach for `List[Int]` — instead, the GC always scans list elements but the tag bit distinguishes pointers from values.

---

## 6. Codegen Changes

### 6.1 FIR Level

FIR currently uses `FirType::I64` for integers. This doesn't change — the tagged value is still an i64. But arithmetic operations change:

**Before:**
```
FirExpr::BinaryOp { left, op: Add, right, result_ty: I64 }
→ Cranelift: iadd left, right
```

**After:**
```
FirExpr::BinaryOp { left, op: Add, right, result_ty: I64 }
→ RuntimeCall: aster_int_add(left, right)
```

All integer arithmetic becomes runtime calls to handle the tag checking and potential promotion. This is the main performance cost.

### 6.2 Optimization: Inline Small-Int Fast Path

For the common case (both operands are small ints, no overflow), the compiler can inline the fast path:

```cranelift
; Check both are small ints (both have low bit = 1)
and a, b -> both_tagged
and both_tagged, 1 -> is_small
brz is_small, slow_path

; Fast path: add raw values, check overflow
; Since both have tag bit 1: (a + b - 1) gives correct tagged result
; (tag bits: 1 + 1 = 10, minus 1 = 01 — preserves tag)
iadd a, b -> sum
isub sum, 1 -> tagged_sum
; Check overflow with signed overflow flag
bvs overflow_path
return tagged_sum

slow_path:
  call aster_int_add(a, b)
overflow_path:
  call aster_int_add(a, b)
```

This optimization is deferred to a later phase. Initial implementation uses runtime calls for all integer arithmetic.

### 6.3 Integer Literals

Integer literals in source code become tagged small ints at compile time:

```
42  →  (42 << 1) | 1  =  85
-1  →  (-1 << 1) | 1  = -1  (all bits set)
0   →  (0 << 1) | 1   =  1
```

Literals that exceed 63 bits become BigInt constants allocated at module load time.

---

## 7. Impact on Existing Features

### 7.1 List[Int]

List elements are i64 slots. Tagged integers fit in i64. No layout change needed.

GC scanning of `List[Int]` elements becomes tag-aware: small ints (low bit 1) are skipped, BigInt pointers (low bit 0) are traced. This replaces the current `OBJ_LIST_HANDLE_NOPTR` / `OBJ_LIST_HANDLE` distinction with a uniform approach.

### 7.2 Pattern Matching

`match` on integers works unchanged — small ints compare directly.

### 7.3 Eq / Ord Protocols

Auto-derived `Eq` and `Ord` for classes with `Int` fields work unchanged at the source level. The runtime comparison functions handle tagged values.

### 7.4 String Interpolation

`"{some_int}"` calls `aster_int_to_string`, which is updated to handle both representations.

### 7.5 Ranges

`1..=100` — range bounds are tagged ints. Range iteration increments with `aster_int_add`. For the vast majority of ranges this stays in the small-int fast path.

### 7.6 Random

`random(max: 100)` — works on small ints. `random()` for BigInt ranges would need thought (deferred).

---

## 8. Performance Considerations

### 8.1 Overhead

- **Small int arithmetic**: one branch (check tag) + one overflow check per operation. On modern CPUs with branch prediction, this is ~1-2 ns overhead per operation.
- **Memory**: small ints use zero extra memory. BigInts use 16 (header) + 8 (sign/len) + 8×N (limbs) bytes.
- **GC pressure**: only BigInts create GC objects. Small-int-only programs have zero GC overhead from integers.

### 8.2 When It Matters

Programs that do tight numeric loops (e.g., pixel processing, audio) pay the branch cost. For Aster's target use cases (scripting, Euler problems, application logic), this is negligible.

If profiling reveals hot integer loops as bottlenecks, future work could add:
- Loop-level specialization (prove all values stay small, emit raw arithmetic)
- Unboxing optimization for local variables with known small range

---

## 9. Implementation Phases

### Phase 1: Runtime BigInt Library
- Implement or integrate a big integer library in the Rust runtime
- Functions: `bigint_add`, `bigint_sub`, `bigint_mul`, `bigint_div`, `bigint_mod`, `bigint_pow`, `bigint_cmp`, `bigint_to_string`
- GC type `OBJ_BIGINT` with mark support
- Test independently of the compiler

### Phase 2: Tagged Integer Representation
- Change all integer literals to tagged form `(val << 1) | 1`
- Update `aster_say_int`, `aster_int_to_string` to untag
- Update comparison operations
- All arithmetic goes through `aster_int_*` runtime calls
- Update `is_gc_payload` to be tag-aware

### Phase 3: Codegen Integration
- FIR integer arithmetic emits `RuntimeCall` to `aster_int_*` instead of raw Cranelift `iadd`/`imul`/etc.
- Integer literals emit tagged constants
- Update all places that read/write raw i64 integer values (list indexing, range bounds, etc.)

### Phase 4: Cleanup and Optimization
- Remove `aster_pow_int` (replaced by `aster_int_pow`)
- Remove `OBJ_LIST_HANDLE_NOPTR` (tag-aware scanning replaces it)
- Inline small-int fast paths in codegen (optional, perf optimization)
- Update AOT C runtime to match

---

## 10. Open Questions

1. **BigInt library choice**: Roll our own minimal implementation, or bind to mini-gmp? Rolling our own is simpler to integrate but more work to get right. Mini-gmp is ~700 lines of C, battle-tested.

2. **Bitwise operations**: `&`, `|`, `^`, `<<`, `>>` on BigInts — what semantics? Ruby extends them naturally. Deferred until we have bitwise ops at all.

3. **Serialization**: How do BigInts appear in TOON output? As strings? As multi-precision literals?

4. **Float-to-Int conversion**: `3.14.to_int()` → truncate to small int. What about `1e100.to_int()`? Promote to BigInt? Error?
