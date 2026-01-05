// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use fionn_core::path::{ParsedPath, PathCache, parse_baseline, parse_original, parse_simd};
use fionn_tape::DsonTape;

fn bench_path_parsing_full(c: &mut Criterion) {
    let paths = build_paths();
    let long_paths = build_long_paths();
    let json = build_json();
    let tape = DsonTape::parse(&json).expect("parse json");

    let mut parse_group = c.benchmark_group("path_parse_full");
    parse_group.bench_function("original", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_original(black_box(path)));
            }
        });
    });

    parse_group.bench_function("baseline", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_baseline(black_box(path)));
            }
        });
    });

    parse_group.bench_function("simd", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_simd(black_box(path)));
            }
        });
    });

    parse_group.bench_function("parsed_path_parse", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(ParsedPath::parse(black_box(path)));
            }
        });
    });

    let cache = PathCache::new();
    prefill_cache(&cache, &paths);
    parse_group.bench_function("cache_hit", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(cache.get_or_parse(black_box(path)));
            }
        });
    });

    parse_group.finish();

    let parsed_paths: Vec<ParsedPath> = long_paths.iter().map(|p| ParsedPath::parse(p)).collect();
    let cache = PathCache::new();
    prefill_cache(&cache, &long_paths);

    let mut resolve_group = c.benchmark_group("path_resolve_full");
    resolve_group.bench_function("resolve_dynamic", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(tape.resolve_path(black_box(path)).unwrap());
            }
        });
    });

    resolve_group.bench_function("resolve_parsed", |b| {
        b.iter_batched(
            Vec::new,
            |mut buffer| {
                for parsed in &parsed_paths {
                    black_box(
                        tape.resolve_parsed_path_with_buffer(parsed, &mut buffer)
                            .unwrap(),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    resolve_group.bench_function("resolve_cached", |b| {
        b.iter(|| {
            for path in &long_paths {
                let parsed = cache.get_or_parse(black_box(path));
                black_box(tape.resolve_parsed_path(&parsed).unwrap());
            }
        });
    });

    resolve_group.finish();
}

fn build_paths() -> Vec<String> {
    vec![
        "user.name",
        "user.age",
        "user.address.city",
        "items[0].name",
        "items[10].price",
        "orders[3].items[1].sku",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn build_long_paths() -> Vec<String> {
    vec![
        "user.address.city",
        "orders[10].items[1].sku",
        "orders[15].items[0].qty",
        "items[20].tags[2]",
        "metadata.timestamp",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn build_json() -> String {
    let mut items = String::new();
    for i in 0..32 {
        if i > 0 {
            items.push(',');
        }
        items.push_str(&format!(
            r#"{{"id":{i},"name":"item{i}","price":{price},"tags":["a","b","c"]}}"#,
            price = (i + 1) * 10
        ));
    }

    let mut orders = String::new();
    for i in 0..16 {
        if i > 0 {
            orders.push(',');
        }
        orders.push_str(&format!(
            r#"{{"id":{i},"items":[{{"sku":"sku{i}a","qty":1}},{{"sku":"sku{i}b","qty":2}}]}}"#,
        ));
    }

    format!(
        r#"{{
  "user": {{"id": 1, "name": "alice", "age": 30, "address": {{"city": "Dublin", "zip": "D01"}}}},
  "items": [{items}],
  "orders": [{orders}],
  "metadata": {{"version": 3, "timestamp": 1700000000}}
}}"#
    )
}

fn prefill_cache(cache: &PathCache, paths: &[String]) {
    for path in paths {
        let _ = cache.get_or_parse(path);
    }
}

criterion_group!(benches, bench_path_parsing_full);
criterion_main!(benches);
