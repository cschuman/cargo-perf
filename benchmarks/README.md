# cargo-perf Benchmarks

These benchmarks demonstrate the real-world cost of anti-patterns that cargo-perf detects.

## Quick Results

Run on Apple M1 Pro, Rust 1.80:

| Anti-pattern | Bad | Good | Speedup |
|--------------|-----|------|---------|
| Regex in loop (1000 iter) | 28ms | 38µs | **737x** |
| Clone String in loop (1000 iter) | 19µs | 0.4µs | **48x** |
| Vec::new + push (1000 items) | 758ns | 430ns | **1.8x** |
| collect().iter() (1000 items) | 77ns | 33ns | **2.3x** |
| String concat in loop (1000 iter) | 1.5µs | 1.5µs | ~1x* |

\* String concat speedup depends on string size. The real cost is allocation pressure and GC overhead, which these micro-benchmarks don't capture.

## Running Benchmarks

```bash
cd benchmarks
cargo bench
```

## Anti-pattern Details

### 1. Regex::new() in Loop

```rust
// BAD: Compiles regex every iteration
for line in lines {
    if Regex::new(r"\d+").unwrap().is_match(line) { ... }
}

// GOOD: Compile once
let re = Regex::new(r"\d+").unwrap();
for line in lines {
    if re.is_match(line) { ... }
}
```

**Why it's slow:** `Regex::new()` parses and compiles the pattern into a finite automaton. This is expensive (~45µs per compile). Doing it in a loop multiplies this cost by iteration count.

### 2. Clone in Loop

```rust
// BAD: Clones String every iteration
for item in items {
    let owned = expensive_data.clone();
    process(owned);
}

// GOOD: Borrow or move clone outside
let owned = expensive_data.clone();
for item in items {
    process(&owned);
}
```

**Why it's slow:** Each `.clone()` on a heap type allocates memory and copies data. In loops, this creates allocation pressure and memory bandwidth issues.

### 3. format! in Loop

```rust
// BAD: Allocates new String each iteration
for i in 0..1000 {
    let s = format!("item_{}", i);
    results.push(s);
}

// GOOD: Reuse buffer
let mut buf = String::with_capacity(20);
for i in 0..1000 {
    buf.clear();
    write!(&mut buf, "item_{}", i).unwrap();
    results.push(buf.clone()); // or use indices
}
```

**Why it's slow:** `format!()` allocates a new `String` every call. In tight loops, this dominates runtime.

### 4. Vec::new() + push without capacity

```rust
// BAD: Multiple reallocations as Vec grows
let mut v = Vec::new();
for i in 0..1000 {
    v.push(i);
}

// GOOD: Pre-allocate
let mut v = Vec::with_capacity(1000);
for i in 0..1000 {
    v.push(i);
}
```

**Why it's slow:** Vec starts with 0 capacity and doubles when full. For 1000 items, this causes ~10 reallocations and copies.

### 5. collect().iter()

```rust
// BAD: Unnecessary intermediate allocation
items.iter().map(|x| x * 2).collect::<Vec<_>>().iter().sum()

// GOOD: Continue the iterator chain
items.iter().map(|x| x * 2).sum()
```

**Why it's slow:** `.collect()` allocates a Vec just to iterate over it again. The allocation is pure waste.
