// KNOWN GAP (precision, not scored): the three capacity rules match their
// constructor with a bare `path_str.ends_with("Vec::new")` (and likewise
// "HashMap::new" / "String::new") with no `::`-segment boundary check, so ANY
// type whose name merely ENDS in the std type's name is mistaken for it:
// `BitVec`, `SmallVec`, `IndexVec`, `MyHashMap`, `MyString`, ...
//
// The consequence is not just a spurious warning but wrong advice — cargo-perf
// tells the author to call `Vec::with_capacity` on a type that may have no such
// constructor. The sibling rules already do this correctly:
// `Self::path_ends_with_boundary` in async_rules.rs and the segment-equality
// check in `RegexInLoopVisitor` both require a `::` boundary.
//
// Confirmed on 2026-09-11: this file reports `vec-no-capacity`,
// `hashmap-no-capacity` and `string-no-capacity` on the three declarations.
//
// When `is_vec_new` / `is_hashmap_new` / `is_string_new` compare the final two
// path segments (or reuse a boundary-aware helper), move this file to
// `tests/corpus/` as a negative fixture guarding all three rules.
struct BitVec;
impl BitVec {
    fn new() -> Self {
        BitVec
    }
    fn push(&mut self, _b: bool) {}
}

struct MyHashMap;
impl MyHashMap {
    fn new() -> Self {
        MyHashMap
    }
    fn insert(&mut self, _k: u32, _v: u32) {}
}

struct MyString;
impl MyString {
    fn new() -> Self {
        MyString
    }
    fn push_str(&mut self, _s: &str) {}
}

fn build(n: usize) {
    let mut bits = BitVec::new();
    let mut map = MyHashMap::new();
    let mut text = MyString::new();
    for i in 0..n {
        bits.push(true);
        map.insert(i as u32, 1);
        text.push_str("x");
    }
}
