//! In-file import / shadow oracle.
//!
//! cargo-perf is a syntactic linter: it has no type inference and cannot ask the
//! compiler where a name resolves. But most of the name-collision false positives
//! (a user `struct Command`, a local `mod fs`, a `use std::fs as sfs` alias, a
//! user `fn unbounded_channel`) *can* be disambiguated from the single file's AST
//! alone — the `use` declarations and the items defined in the file tell us
//! whether a bare leading name is shadowed locally, aliased, or a genuine std
//! path. This oracle collects exactly that evidence, once per file, so rules can
//! gate their matchers on it instead of matching bare names.
//!
//! It deliberately stops at *in-file* resolution. It does not follow `use` into
//! other modules, resolve glob imports to concrete names, or infer receiver /
//! return types — those need real name resolution. Names with no local evidence
//! are reported as [`Origin::Unknown`] and the caller decides the precision /
//! recall tradeoff.

use std::collections::{HashMap, HashSet};
use syn::{Fields, File, Item, ReturnType, Type, UseTree};

/// Where a leading path segment resolves, as far as an in-file scan can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The name is imported from — or written under — the std family
    /// (`std` / `core` / `alloc`).
    Std,
    /// The name is defined in this file (item ident) or aliased by a `use` to a
    /// non-std path — i.e. it is NOT the std item a rule is looking for.
    Local,
    /// No local evidence either way.
    Unknown,
}

/// Per-file map of `use` imports and locally-defined item names.
#[derive(Debug, Default)]
pub struct ImportOracle {
    /// Idents of items defined at file scope (struct/enum/mod/fn/type/trait/…).
    /// These shadow any outer name of the same spelling.
    local_items: HashSet<String>,
    /// Leaf-or-alias -> canonical path string, e.g. `sfs` -> `std::fs`,
    /// `Command` -> `std::process::Command`, `fs` -> `std::fs`.
    use_map: HashMap<String, String>,
    /// Free-fn ident -> whether its declared return type mentions `Arc`/`Rc`.
    /// Lets a `let x = make_shared();` binding be recognised as holding an
    /// Arc/Rc — the same as a direct `Arc::new(..)` — when the factory function
    /// is defined in this file (D9).
    fn_returns_arc_rc: HashMap<String, bool>,
    /// Struct field ident -> whether EVERY field of that name across all structs
    /// in the file has an `Arc`/`Rc` type. A name shared by an Arc field and a
    /// non-Arc field collapses to `false`, so `self.state.clone()` is treated as
    /// a cheap Arc clone only when the field type is unambiguously Arc/Rc (D10).
    field_is_arc_rc: HashMap<String, bool>,
}

impl ImportOracle {
    /// Build the oracle from a parsed file (one AST scan).
    pub fn from_file(file: &File) -> Self {
        let mut oracle = ImportOracle::default();
        for item in &file.items {
            oracle.record_item(item);
        }
        oracle
    }

    fn record_item(&mut self, item: &Item) {
        match item {
            Item::Use(u) => self.record_use_tree(&u.tree, String::new()),
            Item::Struct(s) => {
                self.add_local(&s.ident);
                self.record_fields(&s.fields);
            }
            Item::Enum(e) => self.add_local(&e.ident),
            Item::Mod(m) => self.add_local(&m.ident),
            Item::Fn(f) => {
                self.add_local(&f.sig.ident);
                let returns_arc = return_type_mentions_arc_rc(&f.sig.output);
                self.fn_returns_arc_rc
                    .insert(f.sig.ident.to_string(), returns_arc);
            }
            Item::Type(t) => self.add_local(&t.ident),
            Item::Trait(t) => self.add_local(&t.ident),
            Item::Const(c) => self.add_local(&c.ident),
            Item::Static(s) => self.add_local(&s.ident),
            Item::Union(u) => self.add_local(&u.ident),
            _ => {}
        }
    }

    fn add_local(&mut self, ident: &syn::Ident) {
        self.local_items.insert(ident.to_string());
    }

    /// Record each named field's Arc/Rc-ness. On a name collision, `&=` collapses
    /// the entry to `false` the moment any same-named field is non-Arc, keeping the
    /// predicate conservative (see [`ImportOracle::field_is_arc_rc`]). Tuple and
    /// unit fields have no ident and are skipped.
    fn record_fields(&mut self, fields: &Fields) {
        if let Fields::Named(named) = fields {
            for field in &named.named {
                if let Some(ident) = &field.ident {
                    let is_arc = type_mentions_arc_rc(&field.ty);
                    let entry = self.field_is_arc_rc.entry(ident.to_string()).or_insert(true);
                    *entry &= is_arc;
                }
            }
        }
    }

    /// Walk a `use` tree, accumulating the module prefix, recording each imported
    /// leaf (or its `as` alias) against the full canonical path.
    fn record_use_tree(&mut self, tree: &UseTree, prefix: String) {
        match tree {
            UseTree::Path(p) => {
                let next = join(&prefix, &p.ident.to_string());
                self.record_use_tree(&p.tree, next);
            }
            UseTree::Name(n) => {
                let leaf = n.ident.to_string();
                let canonical = join(&prefix, &leaf);
                self.use_map.insert(leaf, canonical);
            }
            UseTree::Rename(r) => {
                let canonical = join(&prefix, &r.ident.to_string());
                self.use_map.insert(r.rename.to_string(), canonical);
            }
            UseTree::Group(g) => {
                for item in &g.items {
                    self.record_use_tree(item, prefix.clone());
                }
            }
            // Glob (`use std::fs::*`) imports names we can't enumerate; leave
            // them Unknown rather than guessing.
            UseTree::Glob(_) => {}
        }
    }

    /// True if `name` is the ident of an item defined in this file.
    pub fn is_local_item(&self, name: &str) -> bool {
        self.local_items.contains(name)
    }

    /// Rewrite the leading segment of `path_str` through the `use` map, so a
    /// bare or aliased path becomes its canonical form:
    /// `sfs::read_to_string` -> `std::fs::read_to_string`,
    /// `Command::new` -> `std::process::Command::new` (given `use std::process::Command`).
    /// Paths whose leading segment is unknown are returned unchanged.
    pub fn canonicalize(&self, path_str: &str) -> String {
        let (leading, rest) = match path_str.split_once("::") {
            Some((l, r)) => (l, Some(r)),
            None => (path_str, None),
        };
        match self.use_map.get(leading) {
            Some(canonical) => match rest {
                Some(rest) => format!("{canonical}::{rest}"),
                None => canonical.clone(),
            },
            None => path_str.to_string(),
        }
    }

    /// Resolve the origin of a single leading `name`.
    pub fn origin(&self, name: &str) -> Origin {
        if self.local_items.contains(name) {
            return Origin::Local;
        }
        if let Some(canonical) = self.use_map.get(name) {
            return if is_std_root(canonical) {
                Origin::Std
            } else {
                Origin::Local
            };
        }
        Origin::Unknown
    }

    /// True if a file-scope free function named `name` is declared to return an
    /// `Arc`/`Rc`. Unknown names and non-Arc / unit returns are `false` (D9).
    pub fn local_fn_return_mentions_arc_rc(&self, name: &str) -> bool {
        self.fn_returns_arc_rc.get(name).copied().unwrap_or(false)
    }

    /// True if `name` is a struct field whose type is unambiguously `Arc`/`Rc`
    /// across every same-named field in the file. Unknown or ambiguous names are
    /// `false` (D10).
    pub fn local_field_type_mentions_arc_rc(&self, name: &str) -> bool {
        self.field_is_arc_rc.get(name).copied().unwrap_or(false)
    }
}

/// True if a return type is `-> Arc<..>` / `-> Rc<..>` (peeling references).
fn return_type_mentions_arc_rc(output: &ReturnType) -> bool {
    match output {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => type_mentions_arc_rc(ty),
    }
}

/// True if `ty` is an `Arc`/`Rc` — or a reference to one. Matches on the final
/// path segment, so both `Arc<T>` and `std::sync::Arc<T>` qualify. This is a
/// name-level heuristic: a user type literally named `Arc`/`Rc` would match, but
/// that is vanishingly rare and only ever suppresses a clone-in-loop finding.
fn type_mentions_arc_rc(ty: &Type) -> bool {
    match ty {
        Type::Reference(r) => type_mentions_arc_rc(&r.elem),
        Type::Path(p) => p
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Arc" || seg.ident == "Rc"),
        _ => false,
    }
}

/// Join a `::` module prefix with a trailing segment, tolerating an empty prefix.
fn join(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// True if a canonical path is rooted in the std family.
pub fn is_std_root(path: &str) -> bool {
    matches!(
        path.split("::").next(),
        Some("std") | Some("core") | Some("alloc")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oracle(src: &str) -> ImportOracle {
        ImportOracle::from_file(&syn::parse_file(src).expect("parse"))
    }

    #[test]
    fn local_struct_shadows_std_name() {
        let o = oracle("struct Command { x: u8 } fn f() {}");
        assert!(o.is_local_item("Command"));
        assert_eq!(o.origin("Command"), Origin::Local);
    }

    #[test]
    fn local_module_is_recorded() {
        let o = oracle("mod net { pub struct TcpStream; } mod fs { pub fn read() {} }");
        assert!(o.is_local_item("net"));
        assert!(o.is_local_item("fs"));
    }

    #[test]
    fn local_fn_shadows() {
        let o = oracle("fn unbounded_channel() -> u8 { 0 }");
        assert!(o.is_local_item("unbounded_channel"));
    }

    #[test]
    fn simple_use_maps_leaf_to_canonical() {
        let o = oracle("use std::fs;");
        assert_eq!(o.canonicalize("fs::read_to_string"), "std::fs::read_to_string");
        assert_eq!(o.origin("fs"), Origin::Std);
    }

    #[test]
    fn renamed_use_canonicalizes_alias() {
        let o = oracle("use std::fs as sfs;");
        assert_eq!(
            o.canonicalize("sfs::read_to_string"),
            "std::fs::read_to_string"
        );
        assert_eq!(o.origin("sfs"), Origin::Std);
    }

    #[test]
    fn path_use_maps_type() {
        let o = oracle("use std::process::Command;");
        assert_eq!(o.canonicalize("Command::new"), "std::process::Command::new");
        assert_eq!(o.origin("Command"), Origin::Std);
    }

    #[test]
    fn grouped_use_maps_each_leaf() {
        let o = oracle("use std::{fs, io};");
        assert_eq!(o.canonicalize("fs::read"), "std::fs::read");
        assert_eq!(o.canonicalize("io::stdin"), "std::io::stdin");
    }

    #[test]
    fn nested_grouped_use() {
        let o = oracle("use std::sync::{atomic::AtomicUsize, mpsc};");
        assert_eq!(o.origin("AtomicUsize"), Origin::Std);
        assert_eq!(o.canonicalize("mpsc::channel"), "std::sync::mpsc::channel");
    }

    #[test]
    fn glob_import_stays_unknown() {
        let o = oracle("use std::fs::*;");
        assert_eq!(o.origin("read"), Origin::Unknown);
        // No leaf recorded, so canonicalize leaves it alone.
        assert_eq!(o.canonicalize("read"), "read");
    }

    #[test]
    fn non_std_use_is_local() {
        let o = oracle("use mycrate::Command;");
        assert_eq!(o.origin("Command"), Origin::Local);
    }

    #[test]
    fn unknown_name_is_unknown() {
        let o = oracle("fn f() {}");
        assert_eq!(o.origin("Command"), Origin::Unknown);
        assert_eq!(o.canonicalize("Command::new"), "Command::new");
    }

    // --- Arc/Rc factory-fn and field-type recording (D9 / D10) ---

    #[test]
    fn fn_returning_arc_is_recorded() {
        let o = oracle("use std::sync::Arc; fn shared() -> Arc<Config> { todo!() } struct Config;");
        assert!(o.local_fn_return_mentions_arc_rc("shared"));
    }

    #[test]
    fn fn_returning_arc_via_full_path_is_recorded() {
        let o = oracle("fn shared() -> std::sync::Arc<u8> { todo!() }");
        assert!(o.local_fn_return_mentions_arc_rc("shared"));
    }

    #[test]
    fn fn_returning_rc_reference_is_recorded() {
        let o = oracle("fn shared() -> &'static std::rc::Rc<u8> { todo!() }");
        assert!(o.local_fn_return_mentions_arc_rc("shared"));
    }

    #[test]
    fn fn_returning_non_arc_is_not_recorded() {
        let o = oracle("fn make() -> Vec<u8> { vec![] }");
        assert!(!o.local_fn_return_mentions_arc_rc("make"));
    }

    #[test]
    fn unknown_or_unit_fn_return_is_false() {
        let o = oracle("fn f() {}");
        assert!(!o.local_fn_return_mentions_arc_rc("nonexistent"));
        assert!(!o.local_fn_return_mentions_arc_rc("f")); // unit return
    }

    #[test]
    fn struct_arc_field_is_recorded() {
        let o = oracle("use std::sync::Arc; struct S { state: Arc<Inner> } struct Inner;");
        assert!(o.local_field_type_mentions_arc_rc("state"));
    }

    #[test]
    fn struct_non_arc_field_is_not_recorded() {
        let o = oracle("struct S { data: Vec<u8> }");
        assert!(!o.local_field_type_mentions_arc_rc("data"));
    }

    #[test]
    fn field_name_collision_collapses_to_false() {
        // `state` is Arc in one struct but a plain Vec in another: the name is
        // ambiguous, so the oracle refuses to claim it is Arc (conservative). The
        // collapse holds regardless of declaration order.
        let o = oracle("use std::sync::Arc; struct A { state: Arc<u8> } struct B { state: Vec<u8> }");
        assert!(!o.local_field_type_mentions_arc_rc("state"));
        let o2 =
            oracle("use std::sync::Arc; struct B { state: Vec<u8> } struct A { state: Arc<u8> }");
        assert!(!o2.local_field_type_mentions_arc_rc("state"));
    }

    #[test]
    fn tuple_struct_fields_are_ignored() {
        // No named field, so nothing is recorded and the predicate stays false.
        let o = oracle("struct Wrapper(std::sync::Arc<u8>);");
        assert!(!o.local_field_type_mentions_arc_rc("0"));
    }
}
