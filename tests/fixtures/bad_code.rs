// Test fixture with intentional performance anti-patterns
// This file should trigger multiple cargo-perf warnings

use std::fs;
use std::thread;
use std::time::Duration;

// RULE: async-block-in-async
// Blocking calls inside async functions
async fn bad_async_function() {
    // This should trigger async-block-in-async
    let content = std::fs::read_to_string("file.txt").unwrap();

    // This should also trigger
    thread::sleep(Duration::from_secs(1));

    println!("{}", content);
}

// RULE: clone-in-hot-loop
// Clone inside loops
fn clone_in_loop_example(data: &[String]) {
    for item in data {
        // This should trigger clone-in-hot-loop
        let owned = item.clone();
        println!("{}", owned);
    }
}

fn nested_clone_loop(matrix: &[Vec<String>]) {
    for row in matrix {
        for cell in row {
            // Nested loop clone - should definitely flag this
            let owned = cell.clone();
            process(owned);
        }
    }
}

fn process(_s: String) {}

// RULE: regex-in-loop
// Regex::new inside loop
fn regex_in_loop_example(inputs: &[&str]) {
    for input in inputs {
        // This should trigger regex-in-loop
        let re = regex::Regex::new(r"\d+").unwrap();
        if re.is_match(input) {
            println!("Match!");
        }
    }
}

// RULE: collect-then-iterate
// Collecting then immediately iterating
fn collect_then_iterate_example(data: &[i32]) {
    // This should trigger collect-then-iterate
    let doubled: Vec<_> = data.iter().map(|x| x * 2).collect::<Vec<_>>().iter().map(|x| x + 1).collect();
    println!("{:?}", doubled);
}

fn another_collect_iterate(items: &[String]) {
    // Another pattern - collect then for loop
    for item in items.iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().iter() {
        println!("{}", item);
    }
}

// Good code examples (should NOT trigger)
async fn good_async_function() {
    // Using tokio would be correct
    // tokio::fs::read_to_string("file.txt").await
    println!("This is fine");
}

fn good_clone_outside_loop(data: &[String]) {
    let first = data.first().map(|s| s.clone());
    for item in data {
        // Just borrowing, no clone
        println!("{}", item);
    }
    println!("{:?}", first);
}

fn good_iterator_chain(data: &[i32]) -> Vec<i32> {
    // Proper iterator chain without intermediate collect
    data.iter().map(|x| x * 2).map(|x| x + 1).collect()
}

fn main() {
    println!("Test fixture for cargo-perf");
}
