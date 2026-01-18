// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: unused - dataset generators conditionally compiled by format features
#![allow(unused)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
//! Real-world dataset benchmarks
//!
//! Benchmarks using realistic data structures that mimic common use cases:
//! - Package manifests (package.json, Cargo.toml style)
//! - API responses (GitHub, REST APIs)
//! - Configuration files
//! - Log entries
//! - Database exports

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fionn_core::TapeSource;
use fionn_diff::{DiffOptions, json_diff, json_diff_with_options};
use fionn_gron::{GronOptions, gron};
use fionn_tape::DsonTape;
use serde_json::{Value, json};

// =============================================================================
// Realistic Data Generators
// =============================================================================

/// Generate package.json-like structure
fn generate_package_json(deps: usize) -> String {
    let mut dependencies = serde_json::Map::new();
    for i in 0..deps {
        dependencies.insert(
            format!("package-{}", i),
            json!(format!("^{}.{}.0", i / 10, i % 10)),
        );
    }

    let package = json!({
        "name": "my-awesome-project",
        "version": "1.0.0",
        "description": "A sample project for benchmarking",
        "main": "index.js",
        "scripts": {
            "start": "node index.js",
            "build": "webpack --mode production",
            "test": "jest --coverage",
            "lint": "eslint src/",
            "dev": "nodemon index.js"
        },
        "keywords": ["benchmark", "json", "parser", "simd"],
        "author": "Benchmark Runner",
        "license": "MIT",
        "dependencies": dependencies,
        "devDependencies": {
            "jest": "^29.0.0",
            "webpack": "^5.0.0",
            "eslint": "^8.0.0"
        },
        "repository": {
            "type": "git",
            "url": "https://github.com/example/project.git"
        },
        "engines": {
            "node": ">=18.0.0",
            "npm": ">=9.0.0"
        }
    });

    serde_json::to_string(&package).unwrap()
}

/// Generate GitHub API-like response (list of repos)
fn generate_github_repos(count: usize) -> String {
    let repos: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "id": 1_000_000 + i,
                "node_id": format!("MDEwOlJlcG9zaXRvcnl{}", i),
                "name": format!("repo-{}", i),
                "full_name": format!("org/repo-{}", i),
                "private": i % 3 == 0,
                "owner": {
                    "login": "octocat",
                    "id": 1,
                    "avatar_url": "https://github.com/images/error/octocat_happy.gif",
                    "type": "User"
                },
                "html_url": format!("https://github.com/org/repo-{}", i),
                "description": format!("Repository {} - A sample repository for testing", i),
                "fork": i % 5 == 0,
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-06-15T12:30:00Z",
                "pushed_at": "2024-06-15T12:30:00Z",
                "stargazers_count": i * 10,
                "watchers_count": i * 5,
                "forks_count": i * 2,
                "language": (["Rust", "JavaScript", "Python", "Go"][i % 4]),
                "topics": ["api", "benchmark", "testing"],
                "default_branch": "main"
            })
        })
        .collect();

    serde_json::to_string(&repos).unwrap()
}

/// Generate CloudFormation-like configuration
fn generate_cloudformation(resources: usize) -> String {
    let mut resource_map = serde_json::Map::new();

    for i in 0..resources {
        let resource_type = match i % 4 {
            0 => "AWS::EC2::Instance",
            1 => "AWS::S3::Bucket",
            2 => "AWS::Lambda::Function",
            _ => "AWS::DynamoDB::Table",
        };

        let resource = match i % 4 {
            0 => json!({
                "Type": resource_type,
                "Properties": {
                    "ImageId": "ami-12345678",
                    "InstanceType": "t3.micro",
                    "KeyName": "my-key",
                    "SecurityGroupIds": [format!("sg-{:08x}", i)],
                    "Tags": [
                        {"Key": "Name", "Value": format!("Instance-{}", i)},
                        {"Key": "Environment", "Value": "Production"}
                    ]
                }
            }),
            1 => json!({
                "Type": resource_type,
                "Properties": {
                    "BucketName": format!("my-bucket-{}", i),
                    "AccessControl": "Private",
                    "VersioningConfiguration": {"Status": "Enabled"}
                }
            }),
            2 => json!({
                "Type": resource_type,
                "Properties": {
                    "FunctionName": format!("my-function-{}", i),
                    "Runtime": "python3.9",
                    "Handler": "index.handler",
                    "MemorySize": 256,
                    "Timeout": 30
                }
            }),
            _ => json!({
                "Type": resource_type,
                "Properties": {
                    "TableName": format!("my-table-{}", i),
                    "AttributeDefinitions": [
                        {"AttributeName": "id", "AttributeType": "S"}
                    ],
                    "KeySchema": [
                        {"AttributeName": "id", "KeyType": "HASH"}
                    ],
                    "BillingMode": "PAY_PER_REQUEST"
                }
            }),
        };

        resource_map.insert(format!("Resource{}", i), resource);
    }

    let template = json!({
        "AWSTemplateFormatVersion": "2010-09-09",
        "Description": "Sample CloudFormation template",
        "Parameters": {
            "Environment": {
                "Type": "String",
                "Default": "Production",
                "AllowedValues": ["Development", "Staging", "Production"]
            }
        },
        "Resources": resource_map,
        "Outputs": {
            "StackId": {
                "Value": {"Ref": "AWS::StackId"}
            }
        }
    });

    serde_json::to_string(&template).unwrap()
}

/// Generate log entries (JSONL-style but as array)
fn generate_log_entries(count: usize) -> String {
    let entries: Vec<Value> = (0..count)
        .map(|i| {
            let level = ["DEBUG", "INFO", "WARN", "ERROR"][i % 4];
            json!({
                "timestamp": format!("2024-06-15T{:02}:{:02}:{:02}.{}Z", i / 3600 % 24, i / 60 % 60, i % 60, i % 1000),
                "level": level,
                "service": format!("service-{}", i % 5),
                "message": format!("Log message {} - Processing request", i),
                "context": {
                    "request_id": format!("req-{:08x}", i),
                    "user_id": format!("user-{}", i % 100),
                    "trace_id": format!("trace-{:016x}", i as u64 * 12345)
                },
                "metrics": {
                    "duration_ms": i % 1000,
                    "memory_mb": 50 + i % 200
                }
            })
        })
        .collect();

    serde_json::to_string(&entries).unwrap()
}

/// Generate GeoJSON-like data
fn generate_geojson(features: usize) -> String {
    let feature_list: Vec<Value> = (0..features)
        .map(|i| {
            let lon = -180.0 + (i as f64 * 0.1) % 360.0;
            let lat = -90.0 + (i as f64 * 0.05) % 180.0;
            json!({
                "type": "Feature",
                "id": i,
                "geometry": {
                    "type": "Point",
                    "coordinates": [lon, lat]
                },
                "properties": {
                    "name": format!("Location {}", i),
                    "category": (["restaurant", "park", "museum", "hotel"][i % 4]),
                    "rating": (i % 50) as f64 / 10.0,
                    "reviews": i * 5
                }
            })
        })
        .collect();

    let geojson = json!({
        "type": "FeatureCollection",
        "features": feature_list
    });

    serde_json::to_string(&geojson).unwrap()
}

// =============================================================================
// Parsing Benchmarks
// =============================================================================

/// Benchmark: Parse realistic data structures
fn bench_parse_realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/parse");

    // Package.json with varying dependencies
    for deps in [10, 50, 100] {
        let json = generate_package_json(deps);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("package_json", deps), &json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(input)).unwrap();
                black_box(tape.len())
            })
        });
    }

    // GitHub API response
    for repos in [10, 50, 100] {
        let json = generate_github_repos(repos);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("github_repos", repos),
            &json,
            |b, input| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(input)).unwrap();
                    black_box(tape.len())
                })
            },
        );
    }

    // CloudFormation template
    for resources in [10, 50, 100] {
        let json = generate_cloudformation(resources);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("cloudformation", resources),
            &json,
            |b, input| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(input)).unwrap();
                    black_box(tape.len())
                })
            },
        );
    }

    // Log entries
    for entries in [100, 500, 1000] {
        let json = generate_log_entries(entries);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("log_entries", entries),
            &json,
            |b, input| {
                b.iter(|| {
                    let tape = DsonTape::parse(black_box(input)).unwrap();
                    black_box(tape.len())
                })
            },
        );
    }

    // GeoJSON
    for features in [50, 200, 500] {
        let json = generate_geojson(features);
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("geojson", features), &json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(input)).unwrap();
                black_box(tape.len())
            })
        });
    }

    group.finish();
}

// =============================================================================
// Gron Benchmarks
// =============================================================================

/// Benchmark: Gron on realistic data
fn bench_gron_realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/gron");

    let datasets = [
        ("package_json", generate_package_json(50)),
        ("github_repos", generate_github_repos(20)),
        ("cloudformation", generate_cloudformation(20)),
        ("log_entries", generate_log_entries(100)),
        ("geojson", generate_geojson(50)),
    ];

    for (name, json) in &datasets {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("gron", *name), json, |b, input| {
            b.iter(|| {
                let result = gron(black_box(input), &GronOptions::default());
                black_box(result)
            })
        });
    }

    group.finish();
}

// =============================================================================
// Diff Benchmarks
// =============================================================================

/// Benchmark: Diff on realistic config changes
fn bench_diff_realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/diff");

    // Package.json version bump scenario
    {
        let old = generate_package_json(50);
        let mut new_pkg: Value = serde_json::from_str(&old).unwrap();
        // Simulate version bump and dependency update
        new_pkg["version"] = json!("1.1.0");
        if let Some(deps) = new_pkg["dependencies"].as_object_mut() {
            deps.insert("new-package".to_string(), json!("^1.0.0"));
            deps.remove("package-0");
        }
        let new = serde_json::to_string(&new_pkg).unwrap();

        let old_val: Value = serde_json::from_str(&old).unwrap();
        let new_val: Value = serde_json::from_str(&new).unwrap();

        group.throughput(Throughput::Bytes((old.len() + new.len()) as u64));

        group.bench_function("package_json_update", |b| {
            b.iter(|| {
                let patch = json_diff(black_box(&old_val), black_box(&new_val));
                black_box(patch)
            })
        });
    }

    // CloudFormation resource change
    {
        let old = generate_cloudformation(30);
        let mut new_cfg: Value = serde_json::from_str(&old).unwrap();
        // Simulate adding a resource
        if let Some(resources) = new_cfg["Resources"].as_object_mut() {
            resources.insert(
                "NewResource".to_string(),
                json!({
                    "Type": "AWS::SNS::Topic",
                    "Properties": {"TopicName": "my-topic"}
                }),
            );
        }
        let new = serde_json::to_string(&new_cfg).unwrap();

        let old_val: Value = serde_json::from_str(&old).unwrap();
        let new_val: Value = serde_json::from_str(&new).unwrap();

        group.throughput(Throughput::Bytes((old.len() + new.len()) as u64));

        group.bench_function("cloudformation_add_resource", |b| {
            b.iter(|| {
                let patch = json_diff(black_box(&old_val), black_box(&new_val));
                black_box(patch)
            })
        });
    }

    // Log entry comparison (e.g., for testing)
    {
        let old = generate_log_entries(100);
        let new = generate_log_entries(100); // Different content due to timestamps

        let old_val: Value = serde_json::from_str(&old).unwrap();
        let new_val: Value = serde_json::from_str(&new).unwrap();

        group.throughput(Throughput::Bytes((old.len() + new.len()) as u64));

        // Use optimized LCS for arrays
        let options = DiffOptions {
            optimize_arrays: true,
            ..Default::default()
        };

        group.bench_function("log_entries_diff", |b| {
            b.iter(|| {
                let patch =
                    json_diff_with_options(black_box(&old_val), black_box(&new_val), &options);
                black_box(patch)
            })
        });
    }

    group.finish();
}

// =============================================================================
// Skip Operations Benchmarks
// =============================================================================

/// Benchmark: Skip to specific fields in realistic data
fn bench_skip_realistic(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/skip");

    // GitHub repos - skip to specific repo
    {
        let json = generate_github_repos(100);
        let tape = DsonTape::parse(&json).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("github_skip_to_50th", |b| {
            b.iter(|| {
                // Skip to approximately the 50th element
                let skipped = tape.skip_value(black_box(50 * 20)); // Approximate node index
                black_box(skipped)
            })
        });
    }

    // CloudFormation - skip to specific resource
    {
        let json = generate_cloudformation(50);
        let tape = DsonTape::parse(&json).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_function("cloudformation_skip_resources", |b| {
            b.iter(|| {
                // Skip past Parameters to Resources
                let skipped = tape.skip_value(black_box(10));
                black_box(skipped)
            })
        });
    }

    group.finish();
}

// =============================================================================
// Comparison with serde_json
// =============================================================================

/// Benchmark: Compare with serde_json parsing
fn bench_vs_serde(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/vs_serde");

    let datasets = [
        ("package_json_small", generate_package_json(10)),
        ("package_json_large", generate_package_json(100)),
        ("github_repos_small", generate_github_repos(10)),
        ("github_repos_large", generate_github_repos(100)),
        ("geojson_medium", generate_geojson(100)),
    ];

    for (name, json) in &datasets {
        group.throughput(Throughput::Bytes(json.len() as u64));

        // DsonTape parsing
        group.bench_with_input(BenchmarkId::new("dsontape", *name), json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(input)).unwrap();
                black_box(tape.len())
            })
        });

        // serde_json parsing
        group.bench_with_input(BenchmarkId::new("serde_json", *name), json, |b, input| {
            b.iter(|| {
                let value: Value = serde_json::from_str(black_box(input)).unwrap();
                black_box(value)
            })
        });
    }

    group.finish();
}

// =============================================================================
// Memory Efficiency Benchmarks
// =============================================================================

/// Benchmark: Nodes per byte ratio for realistic data
fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world/memory");

    let datasets = [
        ("package_json", generate_package_json(50)),
        ("github_repos", generate_github_repos(50)),
        ("cloudformation", generate_cloudformation(30)),
        ("log_entries", generate_log_entries(200)),
        ("geojson", generate_geojson(100)),
    ];

    for (name, json) in &datasets {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("node_ratio", *name), json, |b, input| {
            b.iter(|| {
                let tape = DsonTape::parse(black_box(input)).unwrap();
                let ratio = tape.len() as f64 / input.len() as f64;
                black_box((tape.len(), ratio))
            })
        });
    }

    group.finish();
}

criterion_group!(
    real_world_benchmarks,
    bench_parse_realistic,
    bench_gron_realistic,
    bench_diff_realistic,
    bench_skip_realistic,
    bench_vs_serde,
    bench_memory_efficiency,
);

criterion_main!(real_world_benchmarks);
