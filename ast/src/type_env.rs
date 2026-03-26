use crate::types::Type;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub ty: Type,
    /// Preserves declaration order — critical for deterministic constructor parameter ordering.
    pub fields: IndexMap<String, Type>,
    pub methods: HashMap<String, Type>,
    pub generic_params: Option<Vec<String>>,
    pub extends: Option<String>,
    /// Trait names this class includes (e.g., ["Eq", "Printable"]).
    pub includes: Vec<String>,
    /// Methods with multiple signatures from parametric trait inclusions.
    /// Maps method name → Vec of function types (e.g., multiple `into()` overloads).
    pub overloaded_methods: HashMap<String, Vec<Type>>,
    /// Parametric trait inclusions with type args: [(trait_name, [type_args])].
    /// Preserves multiple inclusions of the same trait with different args.
    pub parametric_includes: Vec<(String, Vec<Type>)>,
}

impl ClassInfo {
    pub fn new(ty: Type, fields: IndexMap<String, Type>, methods: HashMap<String, Type>) -> Self {
        Self {
            ty,
            fields,
            methods,
            generic_params: None,
            extends: None,
            includes: Vec::new(),
            overloaded_methods: HashMap::new(),
            parametric_includes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitInfo {
    pub name: String,
    pub methods: HashMap<String, Type>,
    pub required_methods: Vec<String>,
    pub generic_params: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumInfo {
    pub name: String,
    pub variants: Vec<String>,
    pub includes: Vec<String>,
}

/// Stores the public exports of an imported module namespace.
#[derive(Debug, Clone, PartialEq)]
pub struct NamespaceInfo {
    pub variables: HashMap<String, Type>,
    pub classes: HashMap<String, ClassInfo>,
    pub traits: HashMap<String, TraitInfo>,
    pub enums: HashMap<String, EnumInfo>,
}

/// Type environment with Rc-shared maps for O(1) scope creation.
/// Maps are shared via Rc and only cloned on mutation (copy-on-write).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeEnv {
    variables: Rc<HashMap<String, Type>>,
    classes: Rc<HashMap<String, ClassInfo>>,
    traits: Rc<HashMap<String, TraitInfo>>,
    enums: Rc<HashMap<String, EnumInfo>>,
    namespaces: Rc<HashMap<String, NamespaceInfo>>,
    pub parent: Option<Rc<TypeEnv>>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            variables: Rc::new(HashMap::new()),
            classes: Rc::new(HashMap::new()),
            traits: Rc::new(HashMap::new()),
            enums: Rc::new(HashMap::new()),
            namespaces: Rc::new(HashMap::new()),
            parent: None,
        }
    }

    /// Create a child scope by consuming this environment. O(1), no HashMap cloning.
    /// The parent data is moved (not cloned) into the child's parent chain,
    /// so no Rc refcounts are bumped and subsequent mutations avoid deep copies.
    pub fn into_child(self) -> TypeEnv {
        TypeEnv {
            variables: Rc::new(HashMap::new()),
            classes: Rc::new(HashMap::new()),
            traits: Rc::new(HashMap::new()),
            enums: Rc::new(HashMap::new()),
            namespaces: Rc::new(HashMap::new()),
            parent: Some(Rc::new(self)),
        }
    }

    /// Enter a child scope in-place. O(1) — moves data, no clone.
    pub fn enter_scope(&mut self) {
        let snapshot = TypeEnv {
            variables: std::mem::take(&mut self.variables),
            classes: std::mem::take(&mut self.classes),
            traits: std::mem::take(&mut self.traits),
            enums: std::mem::take(&mut self.enums),
            namespaces: std::mem::take(&mut self.namespaces),
            parent: self.parent.take(),
        };
        self.parent = Some(Rc::new(snapshot));
    }

    /// Exit the current scope, restoring the parent's state.
    pub fn exit_scope(&mut self) {
        if let Some(parent_rc) = self.parent.take() {
            match Rc::try_unwrap(parent_rc) {
                Ok(parent) => *self = parent,
                Err(rc) => *self = (*rc).clone(),
            }
        }
    }

    pub fn get_var(&self, name: &str) -> Option<&Type> {
        self.variables
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get_var(name)))
    }

    /// Check if `name` exists in any parent scope (not the current one).
    pub fn parent_has_var(&self, name: &str) -> bool {
        self.parent
            .as_ref()
            .is_some_and(|p| p.get_var(name).is_some())
    }

    pub fn set_var(&mut self, name: String, ty: Type) {
        Rc::make_mut(&mut self.variables).insert(name, ty);
    }

    pub fn get_class(&self, name: &str) -> Option<&ClassInfo> {
        self.classes
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get_class(name)))
    }

    pub fn set_class(&mut self, name: String, info: ClassInfo) {
        Rc::make_mut(&mut self.classes).insert(name, info);
    }

    pub fn get_trait(&self, name: &str) -> Option<&TraitInfo> {
        self.traits
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get_trait(name)))
    }

    pub fn set_trait(&mut self, name: String, info: TraitInfo) {
        Rc::make_mut(&mut self.traits).insert(name, info);
    }

    pub fn remove_trait(&mut self, name: &str) {
        Rc::make_mut(&mut self.traits).remove(name);
    }

    pub fn get_enum(&self, name: &str) -> Option<&EnumInfo> {
        self.enums
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get_enum(name)))
    }

    pub fn set_enum(&mut self, name: String, info: EnumInfo) {
        Rc::make_mut(&mut self.enums).insert(name, info);
    }

    pub fn remove_enum(&mut self, name: &str) {
        Rc::make_mut(&mut self.enums).remove(name);
    }

    pub fn get_namespace(&self, name: &str) -> Option<&NamespaceInfo> {
        self.namespaces
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get_namespace(name)))
    }

    pub fn set_namespace(&mut self, name: String, info: NamespaceInfo) {
        Rc::make_mut(&mut self.namespaces).insert(name, info);
    }

    pub fn all_var_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.variables.keys().map(|s| s.as_str()).collect();
        if let Some(ref parent) = self.parent {
            names.extend(parent.all_var_names());
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    fn dummy_class(name: &str) -> ClassInfo {
        ClassInfo::new(
            Type::Custom(name.to_string(), Vec::new()),
            IndexMap::new(),
            HashMap::new(),
        )
    }

    #[test]
    fn new_env_is_empty() {
        let env = TypeEnv::new();
        assert!(env.variables.is_empty());
        assert!(env.classes.is_empty());
        assert!(env.parent.is_none());
    }

    #[test]
    fn set_and_get_var_in_same_env() {
        let mut env = TypeEnv::new();
        env.set_var("x".into(), Type::Int);
        assert_eq!(env.get_var("x"), Some(&Type::Int));
        assert_eq!(env.get_var("y"), None);
    }

    #[test]
    fn child_inherits_parent_var() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);
        let child = parent.into_child();
        assert_eq!(child.get_var("x"), Some(&Type::Int));
    }

    #[test]
    fn child_can_shadow_var() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);
        let mut child = parent.into_child();
        child.set_var("x".into(), Type::Float);
        assert_eq!(child.get_var("x"), Some(&Type::Float));
    }

    #[test]
    fn set_and_get_class_in_same_env() {
        let mut env = TypeEnv::new();
        let info = dummy_class("Foo");
        env.set_class("Foo".into(), info.clone());
        assert_eq!(env.get_class("Foo"), Some(&info));
        assert_eq!(env.get_class("Bar"), None);
    }

    #[test]
    fn child_inherits_parent_class() {
        let mut parent = TypeEnv::new();
        let info = dummy_class("Foo");
        parent.set_class("Foo".into(), info.clone());
        let child = parent.into_child();
        assert_eq!(child.get_class("Foo"), Some(&info));
    }

    #[test]
    fn child_can_shadow_class() {
        let mut parent = TypeEnv::new();
        let info_parent = dummy_class("Foo");
        parent.set_class("Foo".into(), info_parent);
        let mut child = parent.into_child();
        let info_child = dummy_class("FooChild");
        child.set_class("Foo".into(), info_child.clone());
        assert_eq!(child.get_class("Foo"), Some(&info_child));
    }

    #[test]
    fn fields_and_methods_stored_in_classinfo() {
        let mut fields = IndexMap::new();
        fields.insert("x".into(), Type::Int);
        let mut methods = HashMap::new();
        methods.insert("m".into(), Type::Void);
        let info = ClassInfo::new(
            Type::Custom("Point".into(), Vec::new()),
            fields.clone(),
            methods.clone(),
        );
        let mut env = TypeEnv::new();
        env.set_class("Point".into(), info.clone());
        let got = env.get_class("Point").unwrap();
        assert_eq!(got.fields, fields);
        assert_eq!(got.methods, methods);
    }

    #[test]
    fn child_shares_parent_via_rc() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);
        parent.set_var("y".into(), Type::Float);
        let child = parent.into_child();
        // Parent is shared via Rc, not cloned deeply
        assert!(child.parent.is_some());
        assert_eq!(child.get_var("x"), Some(&Type::Int));
        assert_eq!(child.get_var("y"), Some(&Type::Float));
    }

    #[test]
    fn enter_exit_scope_preserves_state() {
        let mut env = TypeEnv::new();
        env.set_var("x".into(), Type::Int);
        env.enter_scope();
        env.set_var("y".into(), Type::Float);
        assert_eq!(env.get_var("x"), Some(&Type::Int)); // inherited
        assert_eq!(env.get_var("y"), Some(&Type::Float)); // local
        env.exit_scope();
        assert_eq!(env.get_var("x"), Some(&Type::Int));
        assert_eq!(env.get_var("y"), None); // gone after exit
    }

    #[test]
    fn into_child_does_not_bump_refcount() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);
        parent.set_var("y".into(), Type::Float);

        // Parent's variables Rc has strong count 1
        assert_eq!(Rc::strong_count(&parent.variables), 1);

        let child = parent.into_child();

        // The moved parent inside child.parent should still have refcount 1
        // (moved, not cloned — no Rc refcount bump)
        let parent_in_child = child.parent.as_ref().unwrap();
        assert_eq!(Rc::strong_count(&parent_in_child.variables), 1);

        // Child can look up parent's vars through the chain
        assert_eq!(child.get_var("x"), Some(&Type::Int));
        assert_eq!(child.get_var("y"), Some(&Type::Float));
    }

    #[test]
    fn into_child_roundtrip_via_exit_scope() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);
        parent.set_class("Foo".into(), dummy_class("Foo"));

        let mut child = parent.into_child();
        child.set_var("y".into(), Type::Float);

        // Child sees both parent and local bindings
        assert_eq!(child.get_var("x"), Some(&Type::Int));
        assert_eq!(child.get_var("y"), Some(&Type::Float));
        assert_eq!(child.get_class("Foo"), Some(&dummy_class("Foo")));

        // exit_scope restores parent state
        child.exit_scope();
        assert_eq!(child.get_var("x"), Some(&Type::Int));
        assert_eq!(child.get_var("y"), None); // child local gone
        assert_eq!(child.get_class("Foo"), Some(&dummy_class("Foo")));
    }

    #[test]
    fn into_child_parent_mutation_no_deep_clone() {
        let mut env = TypeEnv::new();
        env.set_var("a".into(), Type::Int);
        env.set_var("b".into(), Type::Float);

        // Move into child then immediately restore parent
        let mut child = env.into_child();
        child.exit_scope();

        // After roundtrip, Rc should have refcount 1 (no lingering clones)
        assert_eq!(Rc::strong_count(&child.variables), 1);

        // Mutation should NOT trigger deep clone (refcount is 1)
        child.set_var("c".into(), Type::String);
        assert_eq!(child.get_var("a"), Some(&Type::Int));
        assert_eq!(child.get_var("c"), Some(&Type::String));
    }

    #[test]
    fn into_child_shadowing_works() {
        let mut parent = TypeEnv::new();
        parent.set_var("x".into(), Type::Int);

        let mut child = parent.into_child();
        child.set_var("x".into(), Type::Float);

        // Child sees shadowed value
        assert_eq!(child.get_var("x"), Some(&Type::Float));

        // After exit, original value is restored
        child.exit_scope();
        assert_eq!(child.get_var("x"), Some(&Type::Int));
    }

    #[test]
    fn into_child_multi_level_nesting() {
        let mut root = TypeEnv::new();
        root.set_var("a".into(), Type::Int);

        let mut child1 = root.into_child();
        child1.set_var("b".into(), Type::Float);

        let mut child2 = child1.into_child();
        child2.set_var("c".into(), Type::String);

        // child2 sees all three levels
        assert_eq!(child2.get_var("a"), Some(&Type::Int));
        assert_eq!(child2.get_var("b"), Some(&Type::Float));
        assert_eq!(child2.get_var("c"), Some(&Type::String));

        // No refcount bumps at any level
        let p1 = child2.parent.as_ref().unwrap();
        assert_eq!(Rc::strong_count(&p1.variables), 1);
        let p0 = p1.parent.as_ref().unwrap();
        assert_eq!(Rc::strong_count(&p0.variables), 1);

        // Unwind back to root
        child2.exit_scope();
        assert_eq!(child2.get_var("b"), Some(&Type::Float));
        assert_eq!(child2.get_var("c"), None);
        child2.exit_scope();
        assert_eq!(child2.get_var("a"), Some(&Type::Int));
        assert_eq!(child2.get_var("b"), None);
    }
}
