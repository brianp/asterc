use ast::{ClassInfo, Diagnostic, EnumInfo, Stmt, TraitInfo, Type};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Abstracts filesystem access for module resolution.
/// Production code uses FsResolver, tests use VirtualResolver.
pub trait FileResolver {
    /// Given a module path like ["models", "user"], return (source_code, canonical_filename).
    /// Returns None if the module file doesn't exist.
    fn resolve(&self, module_path: &[String]) -> Option<(String, String)>;
}

/// Filesystem-based resolver for production use.
pub struct FsResolver {
    pub root: std::path::PathBuf,
}

impl FileResolver for FsResolver {
    fn resolve(&self, module_path: &[String]) -> Option<(String, String)> {
        let mut path = self.root.clone();
        for segment in module_path {
            path.push(segment);
        }
        path.set_extension("aster");
        let source = std::fs::read_to_string(&path).ok()?;
        let filename = path.to_string_lossy().to_string();
        Some((source, filename))
    }
}

/// HashMap-based resolver for tests. Keys are module paths joined by "/".
pub struct VirtualResolver {
    pub files: HashMap<String, String>,
}

impl FileResolver for VirtualResolver {
    fn resolve(&self, module_path: &[String]) -> Option<(String, String)> {
        let key = module_path.join("/");
        let source = self.files.get(&key)?;
        let filename = format!("{}.aster", key);
        Some((source.clone(), filename))
    }
}

/// The public exports of a compiled module.
#[derive(Debug, Clone)]
pub struct ModuleExports {
    pub variables: HashMap<String, Type>,
    pub classes: HashMap<String, ClassInfo>,
    pub traits: HashMap<String, TraitInfo>,
    pub enums: HashMap<String, EnumInfo>,
}

/// Loads, compiles, and caches modules.
pub struct ModuleLoader {
    resolver: Box<dyn FileResolver>,
    pub(crate) cache: HashMap<String, ModuleExports>,
    in_progress: HashSet<String>,
}

impl ModuleLoader {
    pub fn new(resolver: Box<dyn FileResolver>) -> Self {
        Self {
            resolver,
            cache: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    /// Load and compile a module, returning its exports.
    /// Uses caching to avoid recompilation and tracks in-progress modules for circular detection.
    pub fn load_module(
        loader_rc: &Rc<RefCell<ModuleLoader>>,
        path: &[String],
        use_span: ast::Span,
    ) -> Result<ModuleExports, Diagnostic> {
        let key = path.join("/");

        // Check cache first
        {
            let loader = loader_rc.borrow();
            if let Some(exports) = loader.cache.get(&key) {
                return Ok(exports.clone());
            }
            // Check circular
            if loader.in_progress.contains(&key) {
                return Err(Diagnostic::error(format!(
                    "Circular import detected: module '{}' is already being compiled",
                    key
                ))
                .with_code("M003")
                .with_label(use_span, "circular import here"));
            }
        }

        // Resolve the file
        let (source, filename) = {
            let loader = loader_rc.borrow();
            loader.resolver.resolve(path).ok_or_else(|| {
                Diagnostic::error(format!("Module '{}' not found", key))
                    .with_code("M001")
                    .with_label(use_span, "module not found")
            })?
        };

        // Mark as in-progress
        {
            loader_rc.borrow_mut().in_progress.insert(key.clone());
        }

        // Lex
        let tokens = lexer::lex(&source).map_err(|mut diag| {
            diag.notes
                .push(format!("in imported module '{}'", filename));
            loader_rc.borrow_mut().in_progress.remove(&key);
            diag
        })?;

        // Parse
        let mut parser = parser::Parser::new(tokens);
        let module_ast = parser.parse_module(&filename).map_err(|mut diag| {
            diag.notes
                .push(format!("in imported module '{}'", filename));
            loader_rc.borrow_mut().in_progress.remove(&key);
            diag
        })?;

        // Typecheck with the same module loader
        let mut tc = crate::typechecker::TypeChecker::with_loader(Rc::clone(loader_rc));
        let diagnostics = tc.check_module_all(&module_ast);

        // Remove from in-progress
        {
            loader_rc.borrow_mut().in_progress.remove(&key);
        }

        // If there were errors in the imported module, report the first one
        if let Some(diag) = diagnostics.into_iter().next() {
            let mut d = diag;
            d.notes.push(format!("in imported module '{}'", filename));
            return Err(d);
        }

        // Extract exports: only pub items
        let exports = extract_exports(&module_ast, &tc);

        // Cache
        {
            loader_rc
                .borrow_mut()
                .cache
                .insert(key, exports.clone());
        }

        Ok(exports)
    }
}

/// Extract public exports from a typechecked module.
fn extract_exports(
    module: &ast::Module,
    tc: &crate::typechecker::TypeChecker,
) -> ModuleExports {
    let mut exports = ModuleExports {
        variables: HashMap::new(),
        classes: HashMap::new(),
        traits: HashMap::new(),
        enums: HashMap::new(),
    };

    for stmt in &module.body {
        match stmt {
            Stmt::Let {
                name,
                is_public: true,
                ..
            } => {
                if let Some(ty) = tc.env.get_var(name) {
                    exports.variables.insert(name.clone(), ty);
                }
            }
            Stmt::Class {
                name,
                is_public: true,
                ..
            } => {
                if let Some(info) = tc.env.get_class(name) {
                    exports.classes.insert(name.clone(), info.clone());
                }
                // Also export the constructor function
                if let Some(ctor_ty) = tc.env.get_var(name) {
                    exports.variables.insert(name.clone(), ctor_ty);
                }
            }
            Stmt::Trait {
                name,
                is_public: true,
                ..
            } => {
                if let Some(info) = tc.env.get_trait(name) {
                    exports.traits.insert(name.clone(), info.clone());
                }
            }
            Stmt::Enum {
                name,
                is_public: true,
                ..
            } => {
                if let Some(info) = tc.env.get_enum(name) {
                    exports.enums.insert(name.clone(), info.clone());
                }
                // Also export the enum type variable
                if let Some(ty) = tc.env.get_var(name) {
                    exports.variables.insert(name.clone(), ty);
                }
            }
            // pub use — re-export items from another module
            Stmt::Use {
                is_public: true,
                names,
                ..
            } => {
                // The imported names are already in tc.env from resolve_use.
                // Re-export the specific names or all imported items.
                match names {
                    Some(selected_names) => {
                        // pub use foo { Bar, baz } — re-export specific names
                        for name in selected_names {
                            export_name_from_env(name, tc, &mut exports);
                        }
                    }
                    None => {
                        // pub use foo — re-export all items that were imported.
                        // We need to find what was imported. Since resolve_use injected
                        // all pub items from the source module into tc.env, and we can't
                        // distinguish them from this module's own definitions, we re-load
                        // the source module's exports from cache and merge them.
                        // The source module is identified by path in the Use statement.
                        if let Stmt::Use { path, .. } = stmt {
                            let key = path.join("/");
                            if let Some(loader) = &tc.module_loader {
                                let loader = loader.borrow();
                                if let Some(source_exports) = loader.cache.get(&key) {
                                    for (n, ty) in &source_exports.variables {
                                        exports.variables.insert(n.clone(), ty.clone());
                                    }
                                    for (n, info) in &source_exports.classes {
                                        exports.classes.insert(n.clone(), info.clone());
                                        // Also re-export the constructor
                                        if let Some(ctor) = source_exports.variables.get(n) {
                                            exports.variables.insert(n.clone(), ctor.clone());
                                        }
                                    }
                                    for (n, info) in &source_exports.traits {
                                        exports.traits.insert(n.clone(), info.clone());
                                    }
                                    for (n, info) in &source_exports.enums {
                                        exports.enums.insert(n.clone(), info.clone());
                                        if let Some(ty) = source_exports.variables.get(n) {
                                            exports.variables.insert(n.clone(), ty.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    exports
}

/// Export a single name from the typechecker's environment into the exports.
fn export_name_from_env(
    name: &str,
    tc: &crate::typechecker::TypeChecker,
    exports: &mut ModuleExports,
) {
    if let Some(info) = tc.env.get_class(name) {
        exports.classes.insert(name.to_string(), info.clone());
        // Also export the constructor
        if let Some(ctor) = tc.env.get_var(name) {
            exports.variables.insert(name.to_string(), ctor);
        }
    }
    if let Some(info) = tc.env.get_trait(name) {
        exports.traits.insert(name.to_string(), info.clone());
    }
    if let Some(info) = tc.env.get_enum(name) {
        exports.enums.insert(name.to_string(), info.clone());
        if let Some(ty) = tc.env.get_var(name) {
            exports.variables.insert(name.to_string(), ty);
        }
    }
    if let Some(ty) = tc.env.get_var(name) {
        exports.variables.insert(name.to_string(), ty);
    }
}
