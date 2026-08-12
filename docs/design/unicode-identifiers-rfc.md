# RFC 0001: Unicode Identifier Support

- **Status:** Draft
- **Date:** 2026-03-25

## Summary

This RFC proposes extending the Aster lexer to accept Unicode identifiers, allowing programmers to write variable names, function names, and type names in their native scripts. The language currently restricts identifiers to ASCII (`[a-zA-Z_][a-zA-Z0-9_]*`). This proposal lifts that restriction by adopting Unicode Standard Annex #31 (UAX #31) identifier rules with NFC normalization.

## Motivation

Aster currently requires all source files to be ASCII (with the recent exception of string literal contents). This forces non-English-speaking developers to transliterate identifiers into Latin characters, which reduces readability for the people writing and maintaining that code.

Most modern languages already support Unicode identifiers. Python adopted them in PEP 3131 (Python 3.0, 2008). Rust adopted them in RFC 2457 (Rust 1.53, 2021). Swift has supported them since 1.0. Aster should follow this well-established precedent.

Additionally, Unicode identifiers are required for idiomatic mathematical notation. Names like `\u03b1`, `\u0394t`, or `\u03a3` are clearer than `alpha`, `delta_t`, or `Sigma` in scientific code.

## Detailed Design

### Source Encoding

All Aster source files must be valid UTF-8. The compiler will reject files that are not valid UTF-8 with a clear error message before lexing begins. This is already nearly the case: Rust's `str` type guarantees UTF-8, and `input.lines()` operates on `&str`. The only new requirement is an explicit check (or clear error) when reading source files from disk.

BOM (byte order mark, U+FEFF) at the start of a file should be silently stripped. It serves no purpose in UTF-8 but appears in files edited with certain Windows tools.

### Identifier Rules

Identifiers follow a profile of UAX #31 "Default Identifiers," filtered to match the approach taken by Python and Rust.

**Identifier start characters:**

- Unicode general categories: `Lu`, `Ll`, `Lt`, `Lm`, `Lo`, `Nl`
- The underscore `_` (U+005F)

**Identifier continue characters:**

- Everything allowed as identifier start
- Unicode general categories: `Mn`, `Mc`, `Nd`, `Pc`
- Zero-width joiner (U+200D) and zero-width non-joiner (U+200C), only in specific contexts as defined by UAX #31

In terms of Rust's standard library, `char::is_alphabetic()` covers the start categories, and `char::is_alphanumeric()` covers the continue categories. The `unicode-ident` crate (used by Rust's own compiler and by `proc-macro2`) provides exact UAX #31 tables with fast lookup and should be used instead of rolling our own tables.

**Excluded:** Emoji are not valid in identifiers. The general categories `So` and `Sk` are excluded. This matches Rust's behavior.

**ASCII compatibility:** The ASCII identifier characters `[a-zA-Z0-9_]` are a strict subset of the Unicode categories above. Every existing Aster program remains valid without modification.

### Normalization

All identifiers are normalized to NFC (Canonical Decomposition followed by Canonical Composition) before comparison. Two identifiers that differ only in normalization form are treated as the same identifier. The compiler normalizes during lexing, so the rest of the pipeline (parser, type checker, codegen) never sees non-NFC identifiers.

NFC is the right choice because:

- Most text editors and input methods produce NFC by default.
- Rust and Swift both chose NFC.
- NFC is the W3C's recommended normalization form for the web.

The `unicode-normalization` crate provides NFC normalization and is well-maintained.

**Compiler behavior:** If a source file contains a non-NFC identifier, the compiler normalizes it silently. It does not warn. This matches Rust's behavior and avoids annoying developers whose editors happen to produce NFD or other forms.

### Confusable Detection

Confusable detection (flagging identifiers that look visually similar, like Latin `a` vs. Cyrillic `\u0430`) is explicitly deferred to a future RFC. It is not required for the initial implementation.

Rationale: confusable detection is a security feature relevant to code review and supply-chain attacks. It belongs in a lint pass (similar to `clippy` or a dedicated Aster linter), not in the core lexer. Implementing it in v1 would significantly expand scope without benefiting the primary use case of allowing non-ASCII identifiers in single-script codebases.

When confusable detection is eventually added, it should use Unicode Technical Standard #39 (Security Mechanisms) and likely restrict mixed-script identifiers within a single file or crate.

### Lexer Implementation Changes

The current lexer has several ASCII assumptions that need updating.

**1. Byte-offset cursor replacement**

The lexer currently tracks position with a `col` variable that assumes 1 byte = 1 character. This works for ASCII but breaks for multi-byte UTF-8 characters. The fix is to track byte offsets directly.

Currently, `tok_start = ls + col - char_bytes` computes the byte offset of a token's start. After the change, the lexer should maintain a `byte_offset` cursor that advances by `ch.len_utf8()` for each character consumed. This makes span computation straightforward: record `byte_offset` before consuming the token, then the span is `start_byte_offset..byte_offset`.

**2. Identifier start detection**

Replace:

```rust
ch.is_ascii_alphabetic() || ch == '_'
```

With:

```rust
unicode_ident::is_xid_start(ch) || ch == '_'
```

Or, if using the standard library only:

```rust
ch.is_alphabetic() || ch == '_'
```

The `unicode-ident` crate is preferred because it implements the exact XID_Start and XID_Continue properties from UAX #31, which `char::is_alphabetic()` approximates but does not match exactly.

**3. Identifier continue detection**

Replace:

```rust
ch.is_ascii_alphanumeric() || ch == '_'
```

With:

```rust
unicode_ident::is_xid_continue(ch)
```

Note that `_` has the XID_Continue property, so no special case is needed for continue (only for start, since `_` does not have XID_Start in all Unicode versions).

**4. NFC normalization of collected identifier**

After collecting the characters of an identifier into a `String`, normalize it:

```rust
use unicode_normalization::UnicodeNormalization;
let normalized: String = raw_ident.nfc().collect();
```

Then use `normalized` for keyword lookup and interning.

**5. Keyword matching**

All Aster keywords are ASCII, so keyword lookup continues to work unchanged after NFC normalization (NFC does not alter ASCII characters).

**6. Span tracking throughout the pipeline**

Spans already use byte offsets in the source string. As long as the lexer emits correct byte-offset spans (per change 1 above), the parser, type checker, and codegen require no changes to span handling.

### Impact on Other Components

**Formatter:** The formatter reads and writes source text. Since it operates on token spans that index into the original UTF-8 source, it should work without changes. If the formatter ever needs to measure display width (for alignment), it should use the `unicode-width` crate rather than byte length or char count.

**Parser error messages:** Error messages that quote identifier names will naturally display Unicode characters, since Rust's `format!` and `println!` handle UTF-8. No changes needed, but error messages should be tested with non-ASCII identifiers to verify nothing truncates or garbles them.

**Debugger output:** Debug info (DWARF) encodes identifier names as UTF-8 strings. Cranelift's debug info emission should handle this correctly, but it needs testing with non-ASCII names.

**String interpolation:** The interpolation parser (`{expr}` inside strings) already delegates to the expression parser. As long as the expression parser handles Unicode identifiers, interpolation works automatically.

## Migration

No migration is needed. ASCII is a strict subset of UTF-8, and the ASCII identifier characters are a strict subset of the Unicode identifier characters. Every existing Aster program is valid under the new rules without any changes.

The only behavioral change is that sequences that were previously lexer errors (non-ASCII characters outside string literals) may now be valid identifiers. This cannot break existing programs because those programs could not have compiled before.

## Testing Strategy

**Unit tests for the lexer:**

- Identifiers in Latin extended (`caf\u00e9`, `na\u00efve`)
- Identifiers in CJK (`\u5909\u6570`, `\u51fd\u6570`)
- Identifiers in Cyrillic, Greek, Arabic, Devanagari
- Identifiers starting with `_` followed by non-ASCII continue characters
- Identifiers containing combining marks (`e\u0301` should normalize to `\u00e9` via NFC)
- NFC normalization: two identifiers differing only in normalization form resolve to the same name
- Rejection of emoji in identifiers
- Rejection of bare combining marks at identifier start
- Rejection of control characters and whitespace categories
- Correct byte-offset spans for multi-byte identifiers
- Keywords remain recognized when followed by non-ASCII identifiers (`if \u03b1 > 0:`)

**Integration tests:**

- A complete program using non-ASCII variable names, function names, and type names
- String interpolation with non-ASCII identifiers: `f"{\u5024}"`
- Error messages pointing at multi-byte tokens display correct column positions
- Round-trip through the formatter preserves non-ASCII identifiers exactly

**Fuzz testing:**

- Feed random UTF-8 strings to the lexer and verify it does not panic
- Feed mixed valid/invalid UTF-8 byte sequences to the file reader and verify graceful rejection

## Alternatives Considered

**1. Allow only a curated set of scripts (e.g., Latin, Greek, Cyrillic).**

This would be simpler but arbitrarily excludes developers who write in CJK, Arabic, Devanagari, or other scripts. The UAX #31 approach is already well-specified and avoids this problem.

**2. Use NFKC normalization instead of NFC.**

NFKC (compatibility decomposition) maps characters like `\ufb01` (fi ligature) to `fi` and `\u2126` (ohm sign) to `\u03a9` (omega). Python uses NFKC. Rust and Swift use NFC. NFKC is more aggressive, which can be surprising (e.g., Roman numeral characters mapping to Latin letters). NFC is the more conservative choice and matches the majority of modern languages.

**3. Require identifiers to be single-script.**

This would prevent mixed-script identifiers (e.g., mixing Latin and Cyrillic in one name) at the language level. While this is a useful security property, it conflates two concerns: enabling Unicode identifiers and preventing confusable attacks. The confusable/mixed-script check belongs in a lint tool, not the grammar.

**4. Do nothing. Keep ASCII-only identifiers.**

This is the simplest option but increasingly out of step with modern language design. It imposes an unnecessary burden on non-English-speaking developers.

## Dependencies

New crate dependencies:

- `unicode-ident` (latest version): UAX #31 identifier character tables. Zero dependencies, widely used (it is the most-downloaded crate on crates.io).
- `unicode-normalization` (latest version): NFC normalization. One dependency (`tinyvec`), widely used.

Both crates are maintained and have no `unsafe` code.

## Unresolved Questions

- Should the compiler warn when a single file contains identifiers from multiple Unicode scripts? This is related to confusable detection and may be deferred along with it.
- Should the REPL (if Aster gets one) handle terminal encoding issues, or assume the terminal supports UTF-8?
- Should non-ASCII identifiers be allowed in the public API of published packages, or should that be a lint-level recommendation?
