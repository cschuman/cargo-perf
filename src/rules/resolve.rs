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
use syn::{File, Item, UseTree};

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
            Item::Struct(s) => self.add_local(&s.ident),
            Item::Enum(e) => self.add_local(&e.ident),
            Item::Mod(m) => self.add_local(&m.ident),
            Item::Fn(f) => self.add_local(&f.sig.ident),
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
}
