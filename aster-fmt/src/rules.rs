use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use ast::expr::{
    BinOp, EnumVariant, ErrorCatchPattern, Expr, MatchPattern, Module, Stmt, StringPart, UnaryOp,
};
use ast::types::{Type, TypeConstraint};

use crate::config::FormatConfig;
use crate::doc::*;
use crate::trivia::{self, Comment};

// Magic trailing comma: stores byte offsets of closing brackets/parens
// that are preceded by a trailing comma in the source.
thread_local! {
    static TRAILING_COMMAS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Detect trailing commas from a token stream and store their positions.
/// A trailing comma is a Comma token followed (skipping Newlines/Dedents) by
/// a closing RBracket or RParen.
pub(crate) fn detect_trailing_commas(tokens: &[lexer::Token]) {
    let mut positions = HashSet::new();
    for i in 0..tokens.len() {
        if !matches!(tokens[i].kind, lexer::TokenKind::Comma) {
            continue;
        }
        let mut j = i + 1;
        while j < tokens.len()
            && matches!(
                tokens[j].kind,
                lexer::TokenKind::Newline | lexer::TokenKind::Dedent
            )
        {
            j += 1;
        }
        if j < tokens.len()
            && matches!(
                tokens[j].kind,
                lexer::TokenKind::RBracket | lexer::TokenKind::RParen
            )
        {
            positions.insert(tokens[j].end);
        }
    }
    TRAILING_COMMAS.with(|tc| *tc.borrow_mut() = positions);
}

pub(crate) fn clear_trailing_commas() {
    TRAILING_COMMAS.with(|tc| tc.borrow_mut().clear());
}

fn has_trailing_comma(span_end: usize) -> bool {
    TRAILING_COMMAS.with(|tc| tc.borrow().contains(&span_end))
}

/// Classify an import path into one of three groups.
/// Group 0: stdlib (`std` or `std/...`), Group 1: third-party, Group 2: app/relative.
fn import_group(path: &[String]) -> u8 {
    match path.first().map(|s| s.as_str()) {
        Some("std") => 0,                   // stdlib (including std/cmp, std/fmt, etc.)
        Some(p) if p.starts_with('.') => 2, // relative paths
        _ => 1,                             // third-party (or unknown)
    }
}

/// Format an entire module, with import merging/sorting/grouping.
pub(crate) fn format_module(module: &Module, config: &FormatConfig) -> Doc {
    // Separate imports from other statements
    let mut imports: Vec<&Stmt> = Vec::new();
    let mut others: Vec<&Stmt> = Vec::new();

    for stmt in &module.body {
        if matches!(stmt, Stmt::Use { .. }) {
            imports.push(stmt);
        } else {
            others.push(stmt);
        }
    }

    let mut result_docs: Vec<Doc> = Vec::new();

    if !imports.is_empty() {
        // Merge imports by (is_public, path) → combined names
        let mut merged: BTreeMap<(bool, Vec<String>), Vec<String>> = BTreeMap::new();
        let mut aliases: Vec<&Stmt> = Vec::new(); // imports with aliases can't be merged

        for imp in &imports {
            if let Stmt::Use {
                path,
                names,
                alias,
                is_public,
                ..
            } = imp
            {
                if alias.is_some() || names.is_none() {
                    // Aliased imports and wildcard imports (no names) can't be merged
                    aliases.push(imp);
                    continue;
                }
                let key = (*is_public, path.clone());
                let entry = merged.entry(key).or_default();
                if let Some(ns) = names {
                    for n in ns {
                        if !entry.contains(n) {
                            entry.push(n.clone());
                        }
                    }
                }
            }
        }

        // Sort names within each import
        for names in merged.values_mut() {
            names.sort();
        }

        // Build import docs grouped by category
        struct ImportEntry {
            group: u8,
            path_str: String,
            doc: Doc,
        }

        let mut entries: Vec<ImportEntry> = Vec::new();

        for ((is_public, path), names) in &merged {
            let doc = if names.is_empty() {
                format_use(path, &None, &None, *is_public)
            } else {
                format_use(path, &Some(names.clone()), &None, *is_public)
            };
            entries.push(ImportEntry {
                group: import_group(path),
                path_str: path.join("/"),
                doc,
            });
        }

        // Add aliased imports
        for imp in &aliases {
            if let Stmt::Use {
                path,
                names,
                alias,
                is_public,
                ..
            } = imp
            {
                let doc = format_use(path, names, alias, *is_public);
                entries.push(ImportEntry {
                    group: import_group(path),
                    path_str: path.join("/"),
                    doc,
                });
            }
        }

        // Sort by group, then by path
        entries.sort_by(|a, b| a.group.cmp(&b.group).then(a.path_str.cmp(&b.path_str)));

        // Emit with blank lines between groups
        let mut last_group: Option<u8> = None;
        for entry in &entries {
            if let Some(lg) = last_group
                && lg != entry.group
            {
                result_docs.push(hardline()); // blank line between groups
            }
            result_docs.push(entry.doc.clone());
            last_group = Some(entry.group);
        }
    }

    // Add non-import statements
    for stmt in &others {
        if !result_docs.is_empty() {
            result_docs.push(hardline());
        }
        result_docs.push(format_stmt(stmt, config));
    }

    let body = join_stmts(&result_docs);
    concat(vec![body, hardline()])
}

/// Format a module with comment preservation.
///
/// Comments are extracted from the source and re-inserted at their original
/// positions relative to the AST statements.
pub(crate) fn format_module_with_comments(
    module: &Module,
    config: &FormatConfig,
    comments: &[Comment],
    source: &str,
) -> Doc {
    if comments.is_empty() {
        return format_module(module, config);
    }

    // Get statement spans for comment assignment.
    let stmt_spans: Vec<ast::Span> = module.body.iter().map(stmt_span).collect();
    let assigned = trivia::assign_comments_to_stmts(comments, &stmt_spans, source);

    // Separate imports from other statements (same as format_module).
    let mut imports: Vec<(usize, &Stmt)> = Vec::new();
    let mut others: Vec<(usize, &Stmt)> = Vec::new();

    for (i, stmt) in module.body.iter().enumerate() {
        if matches!(stmt, Stmt::Use { .. }) {
            imports.push((i, stmt));
        } else {
            others.push((i, stmt));
        }
    }

    let mut result_docs: Vec<Doc> = Vec::new();

    if !imports.is_empty() {
        // Same import merging/sorting logic as format_module.
        let mut merged: BTreeMap<(bool, Vec<String>), Vec<String>> = BTreeMap::new();
        let mut aliases: Vec<&Stmt> = Vec::new();
        let mut first_import_idx: Option<usize> = None;

        for &(i, imp) in &imports {
            if first_import_idx.is_none() {
                first_import_idx = Some(i);
            }
            if let Stmt::Use {
                path,
                names,
                alias,
                is_public,
                ..
            } = imp
            {
                if alias.is_some() || names.is_none() {
                    aliases.push(imp);
                    continue;
                }
                let key = (*is_public, path.clone());
                let entry = merged.entry(key).or_default();
                if let Some(ns) = names {
                    for n in ns {
                        if !entry.contains(n) {
                            entry.push(n.clone());
                        }
                    }
                }
            }
        }

        for names in merged.values_mut() {
            names.sort();
        }

        struct ImportEntry {
            group: u8,
            path_str: String,
            doc: Doc,
        }

        let mut entries: Vec<ImportEntry> = Vec::new();

        for ((is_public, path), names) in &merged {
            let doc = if names.is_empty() {
                format_use(path, &None, &None, *is_public)
            } else {
                format_use(path, &Some(names.clone()), &None, *is_public)
            };
            entries.push(ImportEntry {
                group: import_group(path),
                path_str: path.join("/"),
                doc,
            });
        }

        for imp in &aliases {
            if let Stmt::Use {
                path,
                names,
                alias,
                is_public,
                ..
            } = imp
            {
                let doc = format_use(path, names, alias, *is_public);
                entries.push(ImportEntry {
                    group: import_group(path),
                    path_str: path.join("/"),
                    doc,
                });
            }
        }

        entries.sort_by(|a, b| a.group.cmp(&b.group).then(a.path_str.cmp(&b.path_str)));

        // Insert comments before the first import.
        if let Some(idx) = first_import_idx {
            for c in &assigned[idx] {
                result_docs.push(text(c.trim()));
            }
        }

        let mut last_group: Option<u8> = None;
        for entry in &entries {
            if let Some(lg) = last_group
                && lg != entry.group
            {
                result_docs.push(hardline());
            }
            result_docs.push(entry.doc.clone());
            last_group = Some(entry.group);
        }
    }

    for &(i, stmt) in &others {
        if !result_docs.is_empty() {
            result_docs.push(hardline());
        }
        // Insert comments that belong before this statement.
        for c in &assigned[i] {
            result_docs.push(text(c.trim()));
            result_docs.push(hardline());
        }
        result_docs.push(format_stmt(stmt, config));
    }

    let body = join_stmts(&result_docs);
    concat(vec![body, hardline()])
}

/// Extract the span from a statement.
fn stmt_span(stmt: &Stmt) -> ast::Span {
    match stmt {
        Stmt::Let { span, .. }
        | Stmt::Class { span, .. }
        | Stmt::Trait { span, .. }
        | Stmt::Return(_, span)
        | Stmt::Expr(_, span)
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Assignment { span, .. }
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::Use { span, .. }
        | Stmt::Enum { span, .. }
        | Stmt::Const { span, .. } => *span,
    }
}

/// Join top-level statements with newlines between them.
fn join_stmts(docs: &[Doc]) -> Doc {
    let mut result = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        if i > 0 {
            result.push(hardline());
        }
        result.push(d.clone());
    }
    concat(result)
}

/// Format a single statement.
pub fn format_stmt(stmt: &Stmt, config: &FormatConfig) -> Doc {
    match stmt {
        Stmt::Let {
            name,
            type_ann,
            value,
            is_public,
            ..
        } => format_let(name, type_ann, value, *is_public, config),

        Stmt::Class {
            name,
            fields,
            methods,
            is_public,
            generic_params,
            extends,
            includes,
            ..
        } => format_class(
            name,
            fields,
            methods,
            *is_public,
            generic_params,
            extends,
            includes,
            config,
        ),

        Stmt::Trait {
            name,
            methods,
            is_public,
            generic_params,
            ..
        } => format_trait(name, methods, *is_public, generic_params, config),

        Stmt::Return(expr, _) => concat(vec![text("return "), format_expr(expr, config)]),

        Stmt::Expr(expr, _) => format_expr(expr, config),

        Stmt::If {
            cond,
            then_body,
            elif_branches,
            else_body,
            ..
        } => format_if(cond, then_body, elif_branches, else_body, config),

        Stmt::While { cond, body, .. } => format_while(cond, body, config),

        Stmt::For {
            var, iter, body, ..
        } => format_for(var, iter, body, config),

        Stmt::Assignment { target, value, .. } => concat(vec![
            format_expr(target, config),
            text(" = "),
            format_expr(value, config),
        ]),

        Stmt::Break(_) => text("break"),
        Stmt::Continue(_) => text("continue"),

        Stmt::Use {
            path,
            names,
            alias,
            is_public,
            ..
        } => format_use(path, names, alias, *is_public),

        Stmt::Enum {
            name,
            variants,
            methods,
            includes,
            is_public,
            ..
        } => format_enum(name, variants, methods, includes, *is_public, config),

        Stmt::Const {
            name,
            type_ann,
            value,
            is_public,
            ..
        } => {
            let mut parts = Vec::new();
            if *is_public {
                parts.push(text("pub "));
            }
            parts.push(text("const "));
            parts.push(text(name.as_str()));
            if let Some(ty) = type_ann {
                parts.push(text(": "));
                parts.push(format_type(ty));
            }
            parts.push(text(" = "));
            parts.push(format_expr(value, config));
            concat(parts)
        }
    }
}

// ---------------------------------------------------------------------------
// Let / function definitions
// ---------------------------------------------------------------------------

fn format_let(
    name: &str,
    type_ann: &Option<Type>,
    value: &Expr,
    is_public: bool,
    config: &FormatConfig,
) -> Doc {
    if let Expr::Lambda {
        params,
        ret_type,
        body,
        generic_params,
        throws,
        type_constraints,
        defaults,
        ..
    } = value
    {
        return format_function_def(
            name,
            params,
            ret_type,
            body,
            generic_params,
            throws,
            type_constraints,
            defaults,
            is_public,
            config,
        );
    }

    let mut parts = Vec::new();
    if is_public {
        parts.push(text("pub "));
    }
    parts.push(text("let "));
    parts.push(text(name));
    if let Some(ty) = type_ann {
        parts.push(text(": "));
        parts.push(format_type(ty));
    }
    parts.push(text(" = "));
    parts.push(format_expr(value, config));
    concat(parts)
}

#[allow(clippy::too_many_arguments)]
fn format_function_def(
    name: &str,
    params: &[(String, Type)],
    ret_type: &Type,
    body: &[Stmt],
    generic_params: &Option<Vec<String>>,
    throws: &Option<Box<Type>>,
    type_constraints: &[(String, Vec<TypeConstraint>)],
    defaults: &[Option<Expr>],
    is_public: bool,
    config: &FormatConfig,
) -> Doc {
    let mut header = Vec::new();
    if is_public {
        header.push(text("pub "));
    }
    header.push(text("def "));
    // Strip qualified prefix (e.g. "ClassName.method" -> "method")
    let short_name = name.rsplit_once('.').map_or(name, |(_, m)| m);
    header.push(text(short_name));

    if let Some(gp) = generic_params
        && !gp.is_empty()
    {
        header.push(text("["));
        header.push(join(
            gp.iter().map(|g| text(g.as_str())).collect(),
            text(", "),
        ));
        header.push(text("]"));
    }

    // Compute the prefix length up to and including "("
    let prefix = render_doc(&concat(header.clone()), config);
    let paren_col = prefix.len() + 1; // +1 for the "(" we're about to add

    // Render each param as a string for packing
    let param_strs: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, (pname, ptype))| {
            let mut parts = vec![text(pname.as_str())];
            if !matches!(ptype, Type::Inferred) {
                parts.push(text(": "));
                parts.push(format_type(ptype));
            }
            if let Some(Some(default_expr)) = defaults.get(i) {
                parts.push(text(" = "));
                parts.push(format_expr(default_expr, config));
            }
            render_doc(&concat(parts), config)
        })
        .collect();

    let packed = pack_items_str(&param_strs, paren_col, config);
    header.push(text(format!("({})", packed)));

    // throws comes BEFORE -> in Aster syntax
    if let Some(throw_ty) = throws.as_deref() {
        header.push(text(" throws "));
        header.push(format_type(throw_ty));
    }

    if !matches!(ret_type, Type::Void | Type::Inferred) {
        header.push(text(" -> "));
        header.push(format_type(ret_type));
    }

    for (tvar, constraints) in type_constraints {
        for c in constraints {
            match c {
                TypeConstraint::Extends(class) => {
                    header.push(text(format!(" where {} extends {}", tvar, class)));
                }
                TypeConstraint::Includes(trait_name, args) => {
                    header.push(text(format!(" where {} includes {}", tvar, trait_name)));
                    if !args.is_empty() {
                        header.push(text("["));
                        header.push(join(args.iter().map(format_type).collect(), text(", ")));
                        header.push(text("]"));
                    }
                }
            }
        }
    }

    format_block_inner(&concat(header), body, true, config)
}

// ---------------------------------------------------------------------------
// Blocks (indented body after a header)
// ---------------------------------------------------------------------------

/// Format a block without return stripping (used for if/while/for/etc).
fn format_block(header: &Doc, body: &[Stmt], config: &FormatConfig) -> Doc {
    format_block_inner(header, body, false, config)
}

/// Format a block, optionally stripping `return` on the last statement.
/// Only function/method bodies should use `strip_last_return = true`.
fn format_block_inner(
    header: &Doc,
    body: &[Stmt],
    strip_last_return: bool,
    config: &FormatConfig,
) -> Doc {
    if body.is_empty() {
        return header.clone();
    }
    let last_idx = body.len() - 1;
    let body_docs: Vec<Doc> = body
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if strip_last_return
                && i == last_idx
                && let Stmt::Return(expr, _) = s
            {
                return format_expr(expr, config);
            }
            format_stmt(s, config)
        })
        .collect();
    let mut inner = Vec::new();
    for d in body_docs {
        inner.push(hardline());
        inner.push(d);
    }
    concat(vec![header.clone(), indent(concat(inner))])
}

// ---------------------------------------------------------------------------
// Class
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn format_class(
    name: &str,
    fields: &[(String, Type)],
    methods: &[Stmt],
    is_public: bool,
    generic_params: &Option<Vec<String>>,
    extends: &Option<String>,
    includes: &Option<Vec<(String, Vec<Type>)>>,
    config: &FormatConfig,
) -> Doc {
    let mut header = Vec::new();
    if is_public {
        header.push(text("pub "));
    }
    header.push(text("class "));
    header.push(text(name));

    if let Some(gp) = generic_params
        && !gp.is_empty()
    {
        header.push(text("["));
        header.push(join(
            gp.iter().map(|g| text(g.as_str())).collect(),
            text(", "),
        ));
        header.push(text("]"));
    }

    if let Some(base) = extends {
        header.push(text(" extends "));
        header.push(text(base.as_str()));
    }

    if let Some(inc) = includes {
        for (trait_name, args) in inc {
            header.push(text(" includes "));
            header.push(text(trait_name.as_str()));
            if !args.is_empty() {
                header.push(text("["));
                header.push(join(args.iter().map(format_type).collect(), text(", ")));
                header.push(text("]"));
            }
        }
    }

    let mut body_parts = Vec::new();
    for (fname, ftype) in fields {
        body_parts.push(concat(vec![
            text(fname.as_str()),
            text(": "),
            format_type(ftype),
        ]));
    }
    for method in methods {
        body_parts.push(format_stmt(method, config));
    }

    if body_parts.is_empty() {
        concat(header)
    } else {
        let mut inner = Vec::new();
        for d in body_parts {
            inner.push(hardline());
            inner.push(d);
        }
        concat(vec![concat(header), indent(concat(inner))])
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

fn format_trait(
    name: &str,
    methods: &[Stmt],
    is_public: bool,
    generic_params: &Option<Vec<String>>,
    config: &FormatConfig,
) -> Doc {
    let mut header = Vec::new();
    if is_public {
        header.push(text("pub "));
    }
    header.push(text("trait "));
    header.push(text(name));

    if let Some(gp) = generic_params
        && !gp.is_empty()
    {
        header.push(text("["));
        header.push(join(
            gp.iter().map(|g| text(g.as_str())).collect(),
            text(", "),
        ));
        header.push(text("]"));
    }

    format_block(&concat(header), methods, config)
}

// ---------------------------------------------------------------------------
// Enum
// ---------------------------------------------------------------------------

fn format_enum(
    name: &str,
    variants: &[EnumVariant],
    methods: &[Stmt],
    includes: &[(String, Vec<Type>)],
    is_public: bool,
    config: &FormatConfig,
) -> Doc {
    let mut header = Vec::new();
    if is_public {
        header.push(text("pub "));
    }
    header.push(text("enum "));
    header.push(text(name));

    for (trait_name, args) in includes {
        header.push(text(" includes "));
        header.push(text(trait_name.as_str()));
        if !args.is_empty() {
            header.push(text("["));
            header.push(join(args.iter().map(format_type).collect(), text(", ")));
            header.push(text("]"));
        }
    }

    let mut body_parts = Vec::new();
    for variant in variants {
        if variant.fields.is_empty() {
            body_parts.push(text(variant.name.as_str()));
        } else {
            let field_docs: Vec<Doc> = variant
                .fields
                .iter()
                .map(|(fname, ftype)| {
                    concat(vec![text(fname.as_str()), text(": "), format_type(ftype)])
                })
                .collect();
            body_parts.push(concat(vec![
                text(variant.name.as_str()),
                text("("),
                join(field_docs, text(", ")),
                text(")"),
            ]));
        }
    }
    for method in methods {
        body_parts.push(format_stmt(method, config));
    }

    if body_parts.is_empty() {
        concat(header)
    } else {
        let mut inner = Vec::new();
        for d in body_parts {
            inner.push(hardline());
            inner.push(d);
        }
        concat(vec![concat(header), indent(concat(inner))])
    }
}

// ---------------------------------------------------------------------------
// Control flow
// ---------------------------------------------------------------------------

fn format_if(
    cond: &Expr,
    then_body: &[Stmt],
    elif_branches: &[(Expr, Vec<Stmt>)],
    else_body: &[Stmt],
    config: &FormatConfig,
) -> Doc {
    let header = concat(vec![text("if "), format_expr(cond, config)]);
    let mut result = format_block(&header, then_body, config);

    for (elif_cond, elif_body) in elif_branches {
        let elif_header = concat(vec![text("elif "), format_expr(elif_cond, config)]);
        result = concat(vec![
            result,
            hardline(),
            format_block(&elif_header, elif_body, config),
        ]);
    }

    if !else_body.is_empty() {
        let else_header = text("else");
        result = concat(vec![
            result,
            hardline(),
            format_block(&else_header, else_body, config),
        ]);
    }

    result
}

fn format_while(cond: &Expr, body: &[Stmt], config: &FormatConfig) -> Doc {
    let header = concat(vec![text("while "), format_expr(cond, config)]);
    format_block(&header, body, config)
}

fn format_for(var: &str, iter: &Expr, body: &[Stmt], config: &FormatConfig) -> Doc {
    let header = concat(vec![
        text("for "),
        text(var),
        text(" in "),
        format_expr(iter, config),
    ]);
    format_block(&header, body, config)
}

// ---------------------------------------------------------------------------
// Use / imports
// ---------------------------------------------------------------------------

fn format_use(
    path: &[String],
    names: &Option<Vec<String>>,
    alias: &Option<String>,
    is_public: bool,
) -> Doc {
    let mut parts = Vec::new();
    if is_public {
        parts.push(text("pub "));
    }
    parts.push(text("use "));
    parts.push(text(path.join("/")));

    if let Some(ns) = names {
        parts.push(text(" { "));
        parts.push(join(
            ns.iter().map(|n| text(n.as_str())).collect(),
            text(", "),
        ));
        parts.push(text(" }"));
    }

    if let Some(a) = alias {
        parts.push(text(" as "));
        parts.push(text(a.as_str()));
    }

    concat(parts)
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

pub fn format_expr(expr: &Expr, config: &FormatConfig) -> Doc {
    match expr {
        Expr::Int(n, _) => text(n.to_string()),
        Expr::Float(f, _) => text(format_float(*f)),
        Expr::Str(s, _) => {
            // Always use double quotes — the Aster lexer only supports double-quoted strings.
            let q = '"';
            text(format!("{}{}{}", q, escape_string(s), q))
        }
        Expr::Bool(b, _) => text(if *b { "true" } else { "false" }),
        Expr::Nil(_) => text("nil"),
        Expr::Ident(name, _) => text(name.as_str()),

        Expr::Member { object, field, .. } => concat(vec![
            format_expr(object, config),
            text("."),
            text(field.as_str()),
        ]),

        Expr::Lambda {
            params,
            ret_type,
            body,
            generic_params,
            throws,
            type_constraints,
            defaults,
            ..
        } => format_lambda(
            params,
            ret_type,
            body,
            generic_params,
            throws,
            type_constraints,
            defaults,
            config,
        ),

        Expr::Call {
            func, args, span, ..
        } => format_call(func, args, *span, config),

        Expr::BinaryOp {
            left, op, right, ..
        } => format_binop(left, op, right, config),

        Expr::UnaryOp { op, operand, .. } => format_unaryop(op, operand, config),

        Expr::ListLiteral(elems, span) => {
            if elems.is_empty() {
                text("[]")
            } else if has_trailing_comma(span.end) {
                // Magic trailing comma: force vertical layout with trailing comma.
                let elem_docs: Vec<Doc> = elems.iter().map(|e| format_expr(e, config)).collect();
                concat(vec![
                    text("["),
                    indent(concat(vec![
                        hardline(),
                        join(elem_docs, concat(vec![text(","), hardline()])),
                        text(","),
                    ])),
                    hardline(),
                    text("]"),
                ])
            } else {
                let elem_docs: Vec<Doc> = elems.iter().map(|e| format_expr(e, config)).collect();
                group(concat(vec![
                    text("["),
                    indent(concat(vec![
                        softline(),
                        join(elem_docs, concat(vec![text(","), line()])),
                    ])),
                    softline(),
                    text("]"),
                ]))
            }
        }

        Expr::Index { object, index, .. } => concat(vec![
            format_expr(object, config),
            text("["),
            format_expr(index, config),
            text("]"),
        ]),

        Expr::Match {
            scrutinee, arms, ..
        } => format_match(scrutinee, arms, config),

        Expr::AsyncCall { func, args, .. } => {
            concat(vec![text("async "), format_call_inner(func, args, config)])
        }

        Expr::BlockingCall { func, args, .. } => concat(vec![
            text("blocking "),
            format_call_inner(func, args, config),
        ]),

        Expr::Resolve { expr: inner, .. } => {
            concat(vec![text("resolve "), format_expr(inner, config)])
        }

        Expr::DetachedCall { func, args, .. } => concat(vec![
            text("detached async "),
            format_call_inner(func, args, config),
        ]),

        Expr::Propagate(inner, _) => concat(vec![format_expr(inner, config), text("!")]),

        Expr::Throw(inner, _) => concat(vec![text("throw "), format_expr(inner, config)]),

        Expr::ErrorOr {
            expr: inner,
            default,
            ..
        } => concat(vec![
            format_expr(inner, config),
            text("!.or("),
            format_expr(default, config),
            text(")"),
        ]),

        Expr::ErrorOrElse {
            expr: inner,
            handler,
            ..
        } => concat(vec![
            format_expr(inner, config),
            text("!.or_else("),
            format_expr(handler, config),
            text(")"),
        ]),

        Expr::ErrorCatch {
            expr: inner, arms, ..
        } => format_error_catch(inner, arms, config),

        Expr::StringInterpolation { parts, .. } => {
            // Always use double quotes — the Aster lexer only supports double-quoted strings.
            let q = '"';
            let mut s = String::new();
            s.push(q);
            for part in parts {
                match part {
                    StringPart::Literal(lit) => s.push_str(&escape_interp_literal(lit)),
                    StringPart::Expr(inner_expr) => {
                        s.push('{');
                        let rendered = crate::doc::pretty(
                            config.line_width,
                            config.indent_size,
                            &format_expr(inner_expr, config),
                        );
                        s.push_str(&rendered);
                        s.push('}');
                    }
                }
            }
            s.push(q);
            text(s)
        }

        Expr::Map { entries, span } => {
            if entries.is_empty() {
                text("{}")
            } else if has_trailing_comma(span.end) {
                let entry_docs: Vec<Doc> = entries
                    .iter()
                    .map(|(k, v)| {
                        concat(vec![
                            format_expr(k, config),
                            text(": "),
                            format_expr(v, config),
                        ])
                    })
                    .collect();
                concat(vec![
                    text("{"),
                    indent(concat(vec![
                        hardline(),
                        join(entry_docs, concat(vec![text(","), hardline()])),
                        text(","),
                    ])),
                    hardline(),
                    text("}"),
                ])
            } else {
                let entry_docs: Vec<Doc> = entries
                    .iter()
                    .map(|(k, v)| {
                        concat(vec![
                            format_expr(k, config),
                            text(": "),
                            format_expr(v, config),
                        ])
                    })
                    .collect();
                group(concat(vec![
                    text("{"),
                    indent(concat(vec![
                        softline(),
                        join(entry_docs, concat(vec![text(","), line()])),
                    ])),
                    softline(),
                    text("}"),
                ]))
            }
        }

        Expr::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            concat(vec![
                format_expr(start, config),
                text(op),
                format_expr(end, config),
            ])
        }
    }
}

// ---------------------------------------------------------------------------
// Expression helpers
// ---------------------------------------------------------------------------

fn format_float(f: f64) -> String {
    assert!(
        !f.is_nan() && !f.is_infinite(),
        "NaN/Infinity cannot appear in Aster float literals"
    );
    let s = f.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('"', "\\\"")
}

fn escape_interp_literal(s: &str) -> String {
    escape_string(s).replace('{', "\\{").replace('}', "\\}")
}

/// Pack items into lines with paren-alignment wrapping.
///
/// Wraps when either:
/// - Items on the current line exceed 2/3 of `config.line_width`, OR
/// - Total column position would exceed `config.line_width`.
///
/// `align_col` is the column just after the opening `(`.
/// Returns a raw string with embedded newlines and alignment spaces.
fn pack_items_str(items: &[String], align_col: usize, config: &FormatConfig) -> String {
    if items.is_empty() {
        return String::new();
    }
    if items.len() == 1 {
        return items[0].clone();
    }

    let max_width = config.line_width;
    let two_thirds = max_width * 2 / 3;

    let mut lines: Vec<String> = Vec::new();
    let mut line_buf = items[0].clone();
    let mut col = align_col + items[0].len();

    for item in &items[1..] {
        let piece = format!(", {}", item);
        let new_content_len = line_buf.len() + piece.len();
        let new_col = col + piece.len();

        if new_content_len > two_thirds || new_col > max_width {
            line_buf.push(',');
            lines.push(line_buf);
            line_buf = item.clone();
            col = align_col + item.len();
        } else {
            line_buf.push_str(&piece);
            col += piece.len();
        }
    }
    lines.push(line_buf);

    if lines.len() == 1 {
        lines[0].clone()
    } else {
        let align_spaces: String = " ".repeat(align_col);
        lines.join(&format!("\n{}", align_spaces))
    }
}

/// Render a Doc to a flat string for measuring.
fn render_doc(doc: &Doc, config: &FormatConfig) -> String {
    crate::doc::pretty(config.line_width, config.indent_size, doc)
}

#[allow(clippy::too_many_arguments)]
fn format_lambda(
    params: &[(String, Type)],
    ret_type: &Type,
    body: &[Stmt],
    _generic_params: &Option<Vec<String>>,
    _throws: &Option<Box<Type>>,
    _type_constraints: &[(String, Vec<TypeConstraint>)],
    defaults: &[Option<Expr>],
    config: &FormatConfig,
) -> Doc {
    // Single-expression lambda: `(x: Int) -> x + 1`
    if body.len() == 1
        && let Stmt::Expr(expr, _) = &body[0]
    {
        let mut parts = Vec::new();
        parts.push(text("("));
        let param_docs: Vec<Doc> = params
            .iter()
            .enumerate()
            .map(|(i, (pname, ptype))| {
                let mut p = vec![text(pname.as_str())];
                if !matches!(ptype, Type::Inferred) {
                    p.push(text(": "));
                    p.push(format_type(ptype));
                }
                if let Some(Some(default_expr)) = defaults.get(i) {
                    p.push(text(" = "));
                    p.push(format_expr(default_expr, config));
                }
                concat(p)
            })
            .collect();
        parts.push(join(param_docs, text(", ")));
        parts.push(text(")"));
        if !matches!(ret_type, Type::Void | Type::Inferred) {
            parts.push(text(" -> "));
            parts.push(format_type(ret_type));
        }
        parts.push(text(": "));
        parts.push(format_expr(expr, config));
        return group(concat(parts));
    }

    // Multi-statement lambda: (params) -> RetType:
    //     stmt1
    //     stmt2
    let mut parts = Vec::new();
    parts.push(text("("));
    let param_docs: Vec<Doc> = params
        .iter()
        .enumerate()
        .map(|(i, (pname, ptype))| {
            let mut p = vec![text(pname.as_str())];
            if !matches!(ptype, Type::Inferred) {
                p.push(text(": "));
                p.push(format_type(ptype));
            }
            if let Some(Some(default_expr)) = defaults.get(i) {
                p.push(text(" = "));
                p.push(format_expr(default_expr, config));
            }
            concat(p)
        })
        .collect();
    parts.push(join(param_docs, text(", ")));
    parts.push(text(")"));
    if !matches!(ret_type, Type::Void | Type::Inferred) {
        parts.push(text(" -> "));
        parts.push(format_type(ret_type));
    }
    parts.push(text(":"));
    let header = concat(parts);
    format_block(&header, body, config)
}

fn format_call(
    func: &Expr,
    args: &[(String, ast::Span, Expr)],
    span: ast::Span,
    config: &FormatConfig,
) -> Doc {
    format_call_inner_with_span(func, args, Some(span), config)
}

fn format_call_inner(
    func: &Expr,
    args: &[(String, ast::Span, Expr)],
    config: &FormatConfig,
) -> Doc {
    format_call_inner_with_span(func, args, None, config)
}

fn format_call_inner_with_span(
    func: &Expr,
    args: &[(String, ast::Span, Expr)],
    span: Option<ast::Span>,
    config: &FormatConfig,
) -> Doc {
    let func_doc = format_expr(func, config);
    if args.is_empty() {
        return concat(vec![func_doc, text("()")]);
    }

    // Magic trailing comma: force vertical layout for calls with trailing comma.
    if let Some(sp) = span
        && has_trailing_comma(sp.end)
    {
        let arg_docs: Vec<Doc> = args
            .iter()
            .map(|(name, _, expr)| {
                concat(vec![
                    text(name.as_str()),
                    text(": "),
                    format_expr(expr, config),
                ])
            })
            .collect();
        return concat(vec![
            func_doc,
            text("("),
            indent(concat(vec![
                hardline(),
                join(arg_docs, concat(vec![text(","), hardline()])),
                text(","),
            ])),
            hardline(),
            text(")"),
        ]);
    }

    let func_str = render_doc(&func_doc, config);
    let paren_col = func_str.len() + 1; // +1 for "("

    let arg_strs: Vec<String> = args
        .iter()
        .map(|(name, _, expr)| {
            let val = render_doc(&format_expr(expr, config), config);
            format!("{}: {}", name, val)
        })
        .collect();

    let packed = pack_items_str(&arg_strs, paren_col, config);
    concat(vec![func_doc, text(format!("({})", packed))])
}

/// Precedence level for a binary operator (higher = tighter binding).
fn binop_precedence(op: &BinOp) -> u8 {
    match op {
        BinOp::Or => 0,
        BinOp::And => 1,
        BinOp::Eq | BinOp::Neq => 2,
        BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => 3,
        BinOp::Add | BinOp::Sub => 4,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 5,
        BinOp::Pow => 6,
    }
}

fn format_binop(left: &Expr, op: &BinOp, right: &Expr, config: &FormatConfig) -> Doc {
    let op_str = match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Lte => "<=",
        BinOp::Gte => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
    };

    let prec = binop_precedence(op);

    // Parenthesize child if it's a binop with lower precedence, or if it's
    // a right-child with equal precedence and the operator is left-associative
    // (all except Pow which is right-associative).
    let needs_parens = |child: &Expr, is_right: bool| -> bool {
        if let Expr::BinaryOp { op: child_op, .. } = child {
            let child_prec = binop_precedence(child_op);
            if child_prec < prec {
                return true;
            }
            // Right child of left-associative op at same precedence needs parens
            // e.g. a - (b + c)
            if is_right && child_prec == prec && !matches!(op, BinOp::Pow) {
                // Only if the child op differs (a + b + c doesn't need parens)
                return child_op != op;
            }
            // Left child of right-associative Pow at same precedence needs parens
            // e.g. (a ** b) ** c
            if !is_right && child_prec == prec && matches!(op, BinOp::Pow) {
                return true;
            }
        }
        false
    };

    let left_doc = if needs_parens(left, false) {
        concat(vec![text("("), format_expr(left, config), text(")")])
    } else {
        format_expr(left, config)
    };

    let right_doc = if needs_parens(right, true) {
        concat(vec![text("("), format_expr(right, config), text(")")])
    } else {
        format_expr(right, config)
    };

    group(concat(vec![
        left_doc,
        text(" "),
        text(op_str),
        line(),
        right_doc,
    ]))
}

fn format_unaryop(op: &UnaryOp, operand: &Expr, config: &FormatConfig) -> Doc {
    match op {
        UnaryOp::Neg => concat(vec![text("-"), format_expr(operand, config)]),
        UnaryOp::Not => concat(vec![text("not "), format_expr(operand, config)]),
    }
}

fn format_match(scrutinee: &Expr, arms: &[(MatchPattern, Expr)], config: &FormatConfig) -> Doc {
    let header = concat(vec![text("match "), format_expr(scrutinee, config)]);

    let mut inner = Vec::new();
    for (pattern, body) in arms {
        inner.push(hardline());
        inner.push(concat(vec![
            format_match_pattern(pattern),
            text(" => "),
            format_expr(body, config),
        ]));
    }

    concat(vec![header, indent(concat(inner))])
}

fn format_match_pattern(pattern: &MatchPattern) -> Doc {
    match pattern {
        MatchPattern::Literal(expr, _) => format_pattern_literal(expr),
        MatchPattern::Ident(name, _) => text(name.as_str()),
        MatchPattern::Wildcard(_) => text("_"),
        MatchPattern::EnumVariant {
            enum_name, variant, ..
        } => text(format!("{}.{}", enum_name, variant)),
    }
}

fn format_pattern_literal(expr: &Expr) -> Doc {
    match expr {
        Expr::Int(n, _) => text(n.to_string()),
        Expr::Float(f, _) => text(format_float(*f)),
        Expr::Str(s, _) => text(format!("\"{}\"", escape_string(s))),
        Expr::Bool(b, _) => text(if *b { "true" } else { "false" }),
        Expr::Nil(_) => text("nil"),
        _ => text("<expr>"),
    }
}

fn format_error_catch(
    expr: &Expr,
    arms: &[(ErrorCatchPattern, Expr)],
    config: &FormatConfig,
) -> Doc {
    let header = concat(vec![format_expr(expr, config), text("!.catch")]);

    let mut inner = Vec::new();
    for (pattern, body) in arms {
        inner.push(hardline());
        inner.push(concat(vec![
            format_error_catch_pattern(pattern),
            text(" -> "),
            format_expr(body, config),
        ]));
    }

    concat(vec![header, indent(concat(inner))])
}

fn format_error_catch_pattern(pattern: &ErrorCatchPattern) -> Doc {
    match pattern {
        ErrorCatchPattern::Typed {
            error_type, var, ..
        } => concat(vec![
            text(error_type.as_str()),
            text(" "),
            text(var.as_str()),
        ]),
        ErrorCatchPattern::Wildcard(_) => text("_"),
    }
}

// ---------------------------------------------------------------------------
// Type formatting
// ---------------------------------------------------------------------------

pub fn format_type(ty: &Type) -> Doc {
    match ty {
        Type::Int => text("Int"),
        Type::Float => text("Float"),
        Type::Bool => text("Bool"),
        Type::String => text("String"),
        Type::Nil => text("Nil"),
        Type::Void => text("Void"),
        Type::Never => text("Never"),
        Type::Error => text("<error>"),
        Type::Inferred => text("_"),
        Type::List(inner) => concat(vec![text("List["), format_type(inner), text("]")]),
        Type::Map(k, v) => concat(vec![
            text("Map["),
            format_type(k),
            text(", "),
            format_type(v),
            text("]"),
        ]),
        Type::Custom(name, args) => {
            if args.is_empty() {
                text(name.as_str())
            } else {
                concat(vec![
                    text(name.as_str()),
                    text("["),
                    join(args.iter().map(format_type).collect(), text(", ")),
                    text("]"),
                ])
            }
        }
        Type::TypeVar(name, _) => text(name.as_str()),
        Type::Function {
            param_names,
            params,
            ret,
            throws,
            ..
        } => {
            let param_docs: Vec<Doc> = param_names
                .iter()
                .zip(params.iter())
                .map(|(name, ty)| concat(vec![text(name.as_str()), text(": "), format_type(ty)]))
                .collect();
            let mut parts = vec![text("("), join(param_docs, text(", ")), text(")")];
            if let Some(t) = throws {
                parts.push(text(" throws "));
                parts.push(format_type(t));
            }
            parts.push(text(" -> "));
            parts.push(format_type(ret));
            concat(parts)
        }
        Type::Task(inner) => concat(vec![text("Task["), format_type(inner), text("]")]),
        Type::Nullable(inner) => concat(vec![format_type(inner), text("?")]),
    }
}
