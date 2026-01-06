//! Benchmarks demonstrating the cost of anti-patterns detected by cargo-perf.
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use regex::Regex;
use std::fmt::Write;

const ITERATIONS: usize = 1000;

// =============================================================================
// Regex in Loop
// =============================================================================

fn regex_in_loop_bad(lines: &[&str]) -> usize {
    let mut count = 0;
    for line in lines {
        // BAD: Compiles regex every iteration
        if Regex::new(r"\d+").unwrap().is_match(line) {
            count += 1;
        }
    }
    count
}

fn regex_in_loop_good(lines: &[&str]) -> usize {
    // GOOD: Compile once
    let re = Regex::new(r"\d+").unwrap();
    let mut count = 0;
    for line in lines {
        if re.is_match(line) {
            count += 1;
        }
    }
    count
}

fn bench_regex_in_loop(c: &mut Criterion) {
    let lines: Vec<&str> = (0..ITERATIONS)
        .map(|i| if i % 2 == 0 { "abc123" } else { "abcdef" })
        .collect();

    let mut group = c.benchmark_group("regex_in_loop");
    group.bench_function("bad", |b| b.iter(|| regex_in_loop_bad(black_box(&lines))));
    group.bench_function("good", |b| b.iter(|| regex_in_loop_good(black_box(&lines))));
    group.finish();
}

// =============================================================================
// Clone in Loop
// =============================================================================

fn clone_in_loop_bad(data: &String, n: usize) -> Vec<String> {
    let mut results = Vec::with_capacity(n);
    for _ in 0..n {
        // BAD: Clones every iteration
        results.push(data.clone());
    }
    results
}

fn clone_in_loop_good(data: &String, n: usize) -> Vec<&String> {
    let mut results = Vec::with_capacity(n);
    for _ in 0..n {
        // GOOD: Borrow instead
        results.push(data);
    }
    results
}

fn bench_clone_in_loop(c: &mut Criterion) {
    let data = "a]".repeat(100); // 200 byte string

    let mut group = c.benchmark_group("clone_in_loop");
    group.bench_function("bad", |b| {
        b.iter(|| clone_in_loop_bad(black_box(&data), ITERATIONS))
    });
    group.bench_function("good", |b| {
        b.iter(|| clone_in_loop_good(black_box(&data), ITERATIONS))
    });
    group.finish();
}

// =============================================================================
// format! in Loop
// =============================================================================

fn format_in_loop_bad(n: usize) -> Vec<String> {
    let mut results = Vec::with_capacity(n);
    for i in 0..n {
        // BAD: Allocates new String each iteration
        results.push(format!("item_{}", i));
    }
    results
}

fn format_in_loop_good(n: usize) -> Vec<String> {
    let mut results = Vec::with_capacity(n);
    let mut buf = String::with_capacity(20);
    for i in 0..n {
        // GOOD: Reuse buffer
        buf.clear();
        write!(&mut buf, "item_{}", i).unwrap();
        results.push(buf.clone());
    }
    results
}

fn bench_format_in_loop(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_in_loop");
    group.bench_function("bad", |b| b.iter(|| format_in_loop_bad(black_box(ITERATIONS))));
    group.bench_function("good", |b| b.iter(|| format_in_loop_good(black_box(ITERATIONS))));
    group.finish();
}

// =============================================================================
// Vec without capacity
// =============================================================================

fn vec_no_capacity_bad(n: usize) -> Vec<i32> {
    // BAD: Multiple reallocations
    let mut v = Vec::new();
    for i in 0..n {
        v.push(i as i32);
    }
    v
}

fn vec_no_capacity_good(n: usize) -> Vec<i32> {
    // GOOD: Pre-allocate
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(i as i32);
    }
    v
}

fn bench_vec_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("vec_capacity");
    group.bench_function("bad", |b| b.iter(|| vec_no_capacity_bad(black_box(ITERATIONS))));
    group.bench_function("good", |b| b.iter(|| vec_no_capacity_good(black_box(ITERATIONS))));
    group.finish();
}

// =============================================================================
// collect().iter()
// =============================================================================

fn collect_then_iterate_bad(data: &[i32]) -> i32 {
    // BAD: Unnecessary intermediate allocation
    data.iter()
        .map(|x| x * 2)
        .collect::<Vec<_>>()
        .iter()
        .sum()
}

fn collect_then_iterate_good(data: &[i32]) -> i32 {
    // GOOD: Continue the iterator chain
    data.iter().map(|x| x * 2).sum()
}

fn bench_collect_then_iterate(c: &mut Criterion) {
    let data: Vec<i32> = (0..ITERATIONS as i32).collect();

    let mut group = c.benchmark_group("collect_then_iterate");
    group.bench_function("bad", |b| {
        b.iter(|| collect_then_iterate_bad(black_box(&data)))
    });
    group.bench_function("good", |b| {
        b.iter(|| collect_then_iterate_good(black_box(&data)))
    });
    group.finish();
}

// =============================================================================
// String concatenation in loop
// =============================================================================

fn string_concat_bad(parts: &[&str]) -> String {
    let mut result = String::new();
    for part in parts {
        // BAD: Creates new String each iteration
        result = result + part;
    }
    result
}

fn string_concat_good(parts: &[&str]) -> String {
    let mut result = String::new();
    for part in parts {
        // GOOD: Mutates in place
        result.push_str(part);
    }
    result
}

fn bench_string_concat(c: &mut Criterion) {
    let parts: Vec<&str> = (0..ITERATIONS).map(|_| "x").collect();

    let mut group = c.benchmark_group("string_concat");
    group.bench_function("bad", |b| b.iter(|| string_concat_bad(black_box(&parts))));
    group.bench_function("good", |b| b.iter(|| string_concat_good(black_box(&parts))));
    group.finish();
}

criterion_group!(
    benches,
    bench_regex_in_loop,
    bench_clone_in_loop,
    bench_format_in_loop,
    bench_vec_capacity,
    bench_collect_then_iterate,
    bench_string_concat,
);
criterion_main!(benches);
