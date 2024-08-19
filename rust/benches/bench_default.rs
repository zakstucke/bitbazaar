#![allow(unused_imports)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// <--- EXAMPLE:

fn fibonacci(n: u64) -> u64 {
    let mut a = 0;
    let mut b = 1;

    match n {
        0 => b,
        _ => {
            for _ in 0..n {
                let c = a + b;
                a = b;
                b = c;
            }
            b
        }
    }
}

async fn async_fibonacci(n: u64) -> u64 {
    fibonacci(n)
}

// SYNC EXAMPLE
pub fn bench_sync(c: &mut Criterion) {
    c.bench_function("sync: fib 20", |b| b.iter(|| fibonacci(black_box(20))));
}

// ASYNC EXAMPLE
pub fn bench_async(c: &mut Criterion) {
    c.bench_function("async: fib 20", |b| {
        b.to_async(&get_tokio_rt())
            .iter(|| async_fibonacci(black_box(20)))
    });
}

// CUSTOM CONFIG EXAMPLE
pub fn bench_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("small-sample-size");
    group.sample_size(10).significance_level(0.01);
    group.bench_function("config: fib 20", |b| b.iter(|| fibonacci(black_box(20))));
    group.finish();
}

criterion_group!(benches, bench_sync, bench_async, bench_config);
criterion_main!(benches);

fn get_tokio_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
