// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(missing_docs)]
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(unexpected_cfgs)]
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use fionn_core::path::{
    ParsedPath, PathCache, parse_baseline, parse_simd, parse_simd_cutoff_64, parse_simd_cutoff_96,
    parse_simd_cutoff_128,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use fionn_core::path::parse_simd_forced_sse2;

#[cfg(target_arch = "x86_64")]
use fionn_core::path::{parse_simd_forced_avx2, parse_simd_forced_avx512};

fn bench_short_paths(c: &mut Criterion) {
    let paths = [
        "name",
        "user.name",
        "users[0].name",
        "users[123].profile.stats[4].value",
        "items[12].metadata.created_at",
        "root.level1.level2.level3.level4",
    ];

    let mut group = c.benchmark_group("path_parsing");

    group.bench_function("baseline", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_baseline(black_box(path)));
            }
        });
    });

    group.bench_function("simd", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_simd(black_box(path)));
            }
        });
    });

    group.bench_function("simd_cutoff_64", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_simd_cutoff_64(black_box(path)));
            }
        });
    });

    group.bench_function("simd_cutoff_96", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_simd_cutoff_96(black_box(path)));
            }
        });
    });

    group.bench_function("simd_cutoff_128", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(parse_simd_cutoff_128(black_box(path)));
            }
        });
    });

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            group.bench_function("forced_sse2", |b| {
                b.iter(|| {
                    for path in &paths {
                        black_box(parse_simd_forced_sse2(black_box(path)));
                    }
                });
            });
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            group.bench_function("forced_avx2", |b| {
                b.iter(|| {
                    for path in &paths {
                        black_box(parse_simd_forced_avx2(black_box(path)));
                    }
                });
            });
        }

        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            group.bench_function("forced_avx512", |b| {
                b.iter(|| {
                    for path in &paths {
                        black_box(parse_simd_forced_avx512(black_box(path)));
                    }
                });
            });
        }
    }

    drop(group);
}

fn bench_long_paths(c: &mut Criterion) {
    let long_paths = build_long_paths();

    let mut long_group = c.benchmark_group("path_parsing_long");

    long_group.bench_function("baseline", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(parse_baseline(black_box(path)));
            }
        });
    });

    long_group.bench_function("simd", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(parse_simd(black_box(path)));
            }
        });
    });

    long_group.bench_function("simd_cutoff_64", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(parse_simd_cutoff_64(black_box(path)));
            }
        });
    });

    long_group.bench_function("simd_cutoff_96", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(parse_simd_cutoff_96(black_box(path)));
            }
        });
    });

    long_group.bench_function("simd_cutoff_128", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(parse_simd_cutoff_128(black_box(path)));
            }
        });
    });

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            long_group.bench_function("forced_sse2", |b| {
                b.iter(|| {
                    for path in &long_paths {
                        black_box(parse_simd_forced_sse2(black_box(path)));
                    }
                });
            });
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            long_group.bench_function("forced_avx2", |b| {
                b.iter(|| {
                    for path in &long_paths {
                        black_box(parse_simd_forced_avx2(black_box(path)));
                    }
                });
            });
        }

        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            long_group.bench_function("forced_avx512", |b| {
                b.iter(|| {
                    for path in &long_paths {
                        black_box(parse_simd_forced_avx512(black_box(path)));
                    }
                });
            });
        }
    }

    drop(long_group);
}

fn bench_very_long_paths(c: &mut Criterion) {
    let very_long_paths = build_very_long_paths();

    let mut very_long_group = c.benchmark_group("path_parsing_very_long");

    very_long_group.bench_function("baseline", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(parse_baseline(black_box(path)));
            }
        });
    });

    very_long_group.bench_function("simd", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(parse_simd(black_box(path)));
            }
        });
    });

    very_long_group.bench_function("simd_cutoff_64", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(parse_simd_cutoff_64(black_box(path)));
            }
        });
    });

    very_long_group.bench_function("simd_cutoff_96", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(parse_simd_cutoff_96(black_box(path)));
            }
        });
    });

    very_long_group.bench_function("simd_cutoff_128", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(parse_simd_cutoff_128(black_box(path)));
            }
        });
    });

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            very_long_group.bench_function("forced_sse2", |b| {
                b.iter(|| {
                    for path in &very_long_paths {
                        black_box(parse_simd_forced_sse2(black_box(path)));
                    }
                });
            });
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            very_long_group.bench_function("forced_avx2", |b| {
                b.iter(|| {
                    for path in &very_long_paths {
                        black_box(parse_simd_forced_avx2(black_box(path)));
                    }
                });
            });
        }

        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            very_long_group.bench_function("forced_avx512", |b| {
                b.iter(|| {
                    for path in &very_long_paths {
                        black_box(parse_simd_forced_avx512(black_box(path)));
                    }
                });
            });
        }
    }

    drop(very_long_group);
}

fn bench_cache(c: &mut Criterion) {
    let paths = [
        "name",
        "user.name",
        "users[0].name",
        "users[123].profile.stats[4].value",
        "items[12].metadata.created_at",
        "root.level1.level2.level3.level4",
    ];
    let long_paths = build_long_paths();
    let very_long_paths = build_very_long_paths();

    let mut cache_group = c.benchmark_group("path_parsing_cache");
    let cache = PathCache::new();
    prefill_cache(&cache, &paths);
    prefill_cache(&cache, &long_paths);
    prefill_cache(&cache, &very_long_paths);

    cache_group.bench_function("cache_hit_short", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(cache.get_or_parse(black_box(path)));
            }
        });
    });

    cache_group.bench_function("cache_hit_long", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(cache.get_or_parse(black_box(path)));
            }
        });
    });

    cache_group.bench_function("cache_hit_very_long", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(cache.get_or_parse(black_box(path)));
            }
        });
    });

    cache_group.bench_function("parsed_path_parse_short", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(ParsedPath::parse(black_box(path)));
            }
        });
    });

    cache_group.bench_function("parsed_path_parse_long", |b| {
        b.iter(|| {
            for path in &long_paths {
                black_box(ParsedPath::parse(black_box(path)));
            }
        });
    });

    cache_group.bench_function("parsed_path_parse_very_long", |b| {
        b.iter(|| {
            for path in &very_long_paths {
                black_box(ParsedPath::parse(black_box(path)));
            }
        });
    });

    drop(cache_group);
}

fn build_long_paths() -> Vec<String> {
    let mut paths = Vec::new();

    let mut base = String::new();
    for i in 0..16 {
        if i > 0 {
            base.push('.');
        }
        base.push_str("segment");
        base.push_str(&i.to_string());
    }
    paths.push(base);

    let mut with_arrays = String::new();
    for i in 0..8 {
        if i > 0 {
            with_arrays.push('.');
        }
        with_arrays.push_str("items");
        with_arrays.push('[');
        with_arrays.push_str(&(i * 3).to_string());
        with_arrays.push(']');
        with_arrays.push('.');
        with_arrays.push_str("field");
        with_arrays.push_str(&i.to_string());
    }
    paths.push(with_arrays);

    let mut mixed = String::new();
    for i in 0..12 {
        if i > 0 {
            mixed.push('.');
        }
        mixed.push_str("node");
        mixed.push_str(&i.to_string());
        if i % 2 == 0 {
            mixed.push('[');
            mixed.push_str(&(i * 7).to_string());
            mixed.push(']');
        }
    }
    paths.push(mixed);

    paths
}

fn build_very_long_paths() -> Vec<String> {
    let mut paths = Vec::new();

    let mut segments = String::new();
    for i in 0..40 {
        if i > 0 {
            segments.push('.');
        }
        segments.push_str("segment");
        segments.push_str(&i.to_string());
    }
    paths.push(segments);

    let mut arrays = String::new();
    for i in 0..24 {
        if i > 0 {
            arrays.push('.');
        }
        arrays.push_str("items");
        arrays.push('[');
        arrays.push_str(&(i * 5).to_string());
        arrays.push(']');
        arrays.push('.');
        arrays.push_str("field");
        arrays.push_str(&i.to_string());
    }
    paths.push(arrays);

    let mut mixed = String::new();
    for i in 0..32 {
        if i > 0 {
            mixed.push('.');
        }
        mixed.push_str("node");
        mixed.push_str(&i.to_string());
        if i % 3 == 0 {
            mixed.push('[');
            mixed.push_str(&(i * 11).to_string());
            mixed.push(']');
        }
    }
    paths.push(mixed);

    paths
}

fn prefill_cache(cache: &PathCache, paths: &[impl AsRef<str>]) {
    for path in paths {
        let _ = cache.get_or_parse(path.as_ref());
    }
}

criterion_group!(
    benches,
    bench_short_paths,
    bench_long_paths,
    bench_very_long_paths,
    bench_cache
);
criterion_main!(benches);
