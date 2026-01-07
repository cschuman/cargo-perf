//! Benchmarks demonstrating the cost of async anti-patterns detected by cargo-perf.
//!
//! Run with: cargo bench --bench async_patterns

use criterion::{criterion_group, criterion_main, Criterion};
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Semaphore};

const ITERATIONS: usize = 100;
const CHANNEL_SIZE: usize = 100;

// =============================================================================
// Lock Across Await - Shows contention impact
// =============================================================================

async fn lock_across_await_bad(mutex: Arc<Mutex<i32>>, n: usize) {
    for _ in 0..n {
        // BAD: Hold lock across await - blocks other tasks
        let mut guard = mutex.lock().await;
        *guard += 1;
        // Simulate some async work while holding the lock
        tokio::time::sleep(tokio::time::Duration::from_micros(1)).await;
        // Lock released here at end of iteration
    }
}

async fn lock_across_await_good(mutex: Arc<Mutex<i32>>, n: usize) {
    for _ in 0..n {
        // GOOD: Release lock before await
        {
            let mut guard = mutex.lock().await;
            *guard += 1;
        } // Lock released here
        // Async work after releasing lock
        tokio::time::sleep(tokio::time::Duration::from_micros(1)).await;
    }
}

fn bench_lock_across_await(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("lock_across_await");

    group.bench_function("bad_contention", |b| {
        b.to_async(&rt).iter(|| async {
            let mutex = Arc::new(Mutex::new(0));
            let m1 = mutex.clone();
            let m2 = mutex.clone();

            // Run two tasks that contend for the same lock
            let t1 = tokio::spawn(async move { lock_across_await_bad(m1, ITERATIONS / 2).await });
            let t2 = tokio::spawn(async move { lock_across_await_bad(m2, ITERATIONS / 2).await });

            t1.await.unwrap();
            t2.await.unwrap();
        });
    });

    group.bench_function("good_minimal_hold", |b| {
        b.to_async(&rt).iter(|| async {
            let mutex = Arc::new(Mutex::new(0));
            let m1 = mutex.clone();
            let m2 = mutex.clone();

            let t1 = tokio::spawn(async move { lock_across_await_good(m1, ITERATIONS / 2).await });
            let t2 = tokio::spawn(async move { lock_across_await_good(m2, ITERATIONS / 2).await });

            t1.await.unwrap();
            t2.await.unwrap();
        });
    });

    group.finish();
}

// =============================================================================
// Unbounded Channel - Shows memory and backpressure impact
// =============================================================================

async fn unbounded_channel_bad(n: usize) -> usize {
    // BAD: Unbounded channel - producer can overwhelm consumer
    let (tx, mut rx) = mpsc::unbounded_channel::<i32>();

    // Spawn producer that sends rapidly
    let producer = tokio::spawn(async move {
        for i in 0..n {
            let _ = tx.send(i as i32);
        }
    });

    // Consumer with slight delay
    let mut count = 0;
    while let Some(_) = rx.recv().await {
        count += 1;
    }

    producer.await.unwrap();
    count
}

async fn bounded_channel_good(n: usize) -> usize {
    // GOOD: Bounded channel - provides backpressure
    let (tx, mut rx) = mpsc::channel::<i32>(CHANNEL_SIZE);

    let producer = tokio::spawn(async move {
        for i in 0..n {
            // Will await if channel is full - backpressure!
            let _ = tx.send(i as i32).await;
        }
    });

    let mut count = 0;
    while let Some(_) = rx.recv().await {
        count += 1;
    }

    producer.await.unwrap();
    count
}

fn bench_channel_types(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("channel_types");

    group.bench_function("unbounded", |b| {
        b.to_async(&rt)
            .iter(|| async { unbounded_channel_bad(ITERATIONS * 10).await });
    });

    group.bench_function("bounded", |b| {
        b.to_async(&rt)
            .iter(|| async { bounded_channel_good(ITERATIONS * 10).await });
    });

    group.finish();
}

// =============================================================================
// Unbounded Task Spawning - Shows resource exhaustion risk
// =============================================================================

async fn unbounded_spawn_bad(n: usize) -> i32 {
    // BAD: Spawns unlimited concurrent tasks
    let mut handles = Vec::with_capacity(n);

    for i in 0..n {
        handles.push(tokio::spawn(async move {
            // Simulate some work
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
            i as i32
        }));
    }

    let mut sum = 0;
    for handle in handles {
        sum += handle.await.unwrap();
    }
    sum
}

async fn bounded_spawn_good(n: usize) -> i32 {
    // GOOD: Use semaphore to limit concurrency
    let semaphore = Arc::new(Semaphore::new(10)); // Max 10 concurrent tasks
    let mut handles = Vec::with_capacity(n);

    for i in 0..n {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        handles.push(tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
            drop(permit); // Release permit when done
            i as i32
        }));
    }

    let mut sum = 0;
    for handle in handles {
        sum += handle.await.unwrap();
    }
    sum
}

async fn buffered_stream_good(n: usize) -> i32 {
    // GOOD: Use buffer_unordered for natural concurrency limiting
    stream::iter(0..n)
        .map(|i| async move {
            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
            i as i32
        })
        .buffer_unordered(10)
        .fold(0i32, |acc, x| async move { acc + x })
        .await
}

fn bench_spawn_patterns(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("spawn_patterns");

    group.bench_function("unbounded_spawn", |b| {
        b.to_async(&rt)
            .iter(|| async { unbounded_spawn_bad(ITERATIONS).await });
    });

    group.bench_function("semaphore_bounded", |b| {
        b.to_async(&rt)
            .iter(|| async { bounded_spawn_good(ITERATIONS).await });
    });

    group.bench_function("buffer_unordered", |b| {
        b.to_async(&rt)
            .iter(|| async { buffered_stream_good(ITERATIONS).await });
    });

    group.finish();
}

// =============================================================================
// Mutex Lock in Loop - Shows lock acquisition overhead
// =============================================================================

async fn mutex_in_loop_bad(mutex: Arc<Mutex<i32>>, n: usize) {
    // BAD: Acquires lock every iteration
    for _ in 0..n {
        let mut guard = mutex.lock().await;
        *guard += 1;
    }
}

async fn mutex_outside_loop_good(mutex: Arc<Mutex<i32>>, n: usize) {
    // GOOD: Acquire lock once
    let mut guard = mutex.lock().await;
    for _ in 0..n {
        *guard += 1;
    }
}

fn bench_mutex_in_loop(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("mutex_acquisition");

    group.bench_function("acquire_in_loop", |b| {
        b.to_async(&rt).iter(|| async {
            let mutex = Arc::new(Mutex::new(0));
            mutex_in_loop_bad(mutex, ITERATIONS * 10).await;
        });
    });

    group.bench_function("acquire_once", |b| {
        b.to_async(&rt).iter(|| async {
            let mutex = Arc::new(Mutex::new(0));
            mutex_outside_loop_good(mutex, ITERATIONS * 10).await;
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_lock_across_await,
    bench_channel_types,
    bench_spawn_patterns,
    bench_mutex_in_loop,
);
criterion_main!(benches);
