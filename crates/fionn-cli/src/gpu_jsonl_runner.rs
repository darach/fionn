// SPDX-License-Identifier: MIT OR Apache-2.0
//! GPU-accelerated JSONL batch processor.
//!
//! This binary provides high-performance JSONL processing with optional GPU acceleration.

use std::time::{Duration, Instant};

// #[cfg(feature = "gpu")]
// use fionn_stream::skiptape::GpuTapeBuilder;
use fionn_stream::skiptape::CompiledSchema;
use fionn_stream::skiptape::jsonl::{PreScanMode, SimdJsonlBatchProcessor};
use memchr::memchr_iter;

fn main() {
    let config = Config::from_args();
    println!("gpu-jsonl-runner: {}", config.summary());

    if config.run_windows {
        match run_windows_driver(&config) {
            Ok(()) => return,
            Err(err) => {
                eprintln!("Windows driver run failed: {err}");
                return;
            }
        }
    }

    if config.windows_only {
        eprintln!("--windows-only set, but --run-windows was not used.");
        std::process::exit(1);
    }

    let data = if let Some(target_bytes) = config.target_bytes {
        build_jsonl_target_bytes(target_bytes, config.match_ratio, config.payload_bytes)
    } else {
        build_jsonl(config.lines, config.match_ratio, config.payload_bytes)
    };
    println!("data_size_bytes={}", data.len());
    let schema = build_schema();

    if config.cpu
        && let Err(err) = run_variant_set("cpu", PreScanMode::CpuOnly, &data, &schema, &config)
    {
        eprintln!("CPU run failed: {err}");
        std::process::exit(1);
    }

    if config.gpu
        && let Err(err) = run_variant_set("gpu", PreScanMode::Gpu, &data, &schema, &config)
    {
        eprintln!("GPU run failed: {err}");
        std::process::exit(1);
    }
}

#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
struct Config {
    lines: usize,
    match_ratio: f64,
    runs: u32,
    warmup: u32,
    target_bytes: Option<usize>,
    payload_bytes: usize,
    cpu: bool,
    gpu: bool,
    run_windows: bool,
    windows_only: bool,
    windows_exe: Option<String>,
}

impl Config {
    fn from_args() -> Self {
        let mut lines = 50_000usize;
        let mut match_ratio = 0.5f64;
        let mut runs = 10u32;
        let mut warmup = 3u32;
        let mut target_bytes = None;
        let mut payload_bytes = 0usize;
        let mut cpu = true;
        let mut gpu = true;
        let mut run_windows = false;
        let mut windows_only = false;
        let mut windows_exe = None;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--lines" => {
                    if let Some(value) = args.next() {
                        lines = value.parse().unwrap_or(lines);
                    }
                }
                "--match-ratio" => {
                    if let Some(value) = args.next() {
                        match_ratio = value.parse().unwrap_or(match_ratio);
                    }
                }
                "--runs" => {
                    if let Some(value) = args.next() {
                        runs = value.parse().unwrap_or(runs);
                    }
                }
                "--target-bytes" => {
                    if let Some(value) = args.next() {
                        target_bytes = parse_bytes(&value);
                        if target_bytes.is_none() {
                            eprintln!("Invalid --target-bytes value: {value}");
                            print_usage_and_exit();
                        }
                    } else {
                        print_usage_and_exit();
                    }
                }
                "--payload-bytes" => {
                    if let Some(value) = args.next() {
                        payload_bytes = value.parse().unwrap_or(payload_bytes);
                    }
                }
                "--warmup" => {
                    if let Some(value) = args.next() {
                        warmup = value.parse().unwrap_or(warmup);
                    }
                }
                "--cpu-only" => {
                    cpu = true;
                    gpu = false;
                }
                "--gpu-only" => {
                    cpu = false;
                    gpu = true;
                }
                "--run-windows" => {
                    run_windows = true;
                }
                "--windows-only" => {
                    windows_only = true;
                }
                "--windows-exe" => {
                    if let Some(value) = args.next() {
                        windows_exe = Some(value);
                    } else {
                        print_usage_and_exit();
                    }
                }
                #[allow(clippy::match_same_arms)]
                "--help" | "-h" => {
                    print_usage_and_exit();
                }
                _ => {
                    print_usage_and_exit();
                }
            }
        }

        Self {
            lines,
            match_ratio: match_ratio.clamp(0.0, 1.0),
            runs: runs.max(1),
            warmup,
            target_bytes,
            payload_bytes,
            cpu,
            gpu,
            run_windows,
            windows_only,
            windows_exe,
        }
    }

    fn summary(&self) -> String {
        format!(
            "lines={}, match_ratio={:.2}, runs={}, warmup={}, payload_bytes={}, cpu={}, gpu={}, run_windows={}",
            self.lines,
            self.match_ratio,
            self.runs,
            self.warmup,
            self.payload_bytes,
            self.cpu,
            self.gpu,
            self.run_windows
        )
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage: gpu-jsonl-runner [--lines N] [--match-ratio R] [--runs N] [--warmup N] \\\n\
         [--target-bytes BYTES] [--payload-bytes N] [--cpu-only|--gpu-only] [--run-windows] [--windows-only] [--windows-exe PATH]\n\
         Defaults: --lines 50000 --match-ratio 0.5 --runs 10 --warmup 3"
    );
    std::process::exit(1);
}

fn build_schema() -> CompiledSchema {
    CompiledSchema::compile(&[
        "name".to_string(),
        "age".to_string(),
        "active".to_string(),
        "score".to_string(),
    ])
    .expect("schema compile")
}

#[allow(clippy::cast_precision_loss)]
fn build_jsonl(lines: usize, match_ratio: f64, payload_bytes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let payload = payload_string(payload_bytes);
    for i in 0..lines {
        let matched = (i as f64 / lines as f64) < match_ratio;
        if matched {
            let line = format!(
                "{{\"name\":\"user_{i}\",\"age\":{age},\"active\":{active},\"score\":{score},\"extra\":\"payload_{i}\",\"blob\":\"{payload}\"}}\n",
                age = 20 + (i % 50),
                active = if i % 2 == 0 { "true" } else { "false" },
                score = (i % 100) as f64 * 1.01
            );
            out.extend_from_slice(line.as_bytes());
        } else {
            let line = format!("{{\"id\":{i},\"payload\":\"skip_{i}\",\"blob\":\"{payload}\"}}\n");
            out.extend_from_slice(line.as_bytes());
        }
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn build_jsonl_target_bytes(
    target_bytes: usize,
    match_ratio: f64,
    payload_bytes: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(target_bytes);
    let payload = payload_string(payload_bytes);
    let mut i = 0usize;
    while out.len() < target_bytes {
        let matched = (i as f64 % 100.0) < (match_ratio * 100.0);
        if matched {
            let line = format!(
                "{{\"name\":\"user_{i}\",\"age\":{age},\"active\":{active},\"score\":{score},\"extra\":\"payload_{i}\",\"blob\":\"{payload}\"}}\n",
                age = 20 + (i % 50),
                active = if i.is_multiple_of(2) { "true" } else { "false" },
                score = (i % 100) as f64 * 1.01
            );
            out.extend_from_slice(line.as_bytes());
        } else {
            let line = format!("{{\"id\":{i},\"payload\":\"skip_{i}\",\"blob\":\"{payload}\"}}\n");
            out.extend_from_slice(line.as_bytes());
        }
        i = i.wrapping_add(1);
    }
    out
}

fn payload_string(payload_bytes: usize) -> String {
    if payload_bytes == 0 {
        return String::new();
    }
    let mut s = String::with_capacity(payload_bytes);
    while s.len() < payload_bytes {
        s.push('x');
    }
    s
}

fn run_variant_set(
    label: &str,
    mode: PreScanMode,
    data: &[u8],
    schema: &CompiledSchema,
    config: &Config,
) -> Result<(), String> {
    println!("Running {label} variants (mode={mode:?})");
    let mut processor = SimdJsonlBatchProcessor::new();
    if let Err(err) = processor.set_prescan_mode(mode) {
        return Err(err.to_string());
    }
    processor.set_gpu_min_bytes(0);

    let baseline = bench_variant("baseline_serde", config, || baseline_serde(data));
    let raw = bench_variant("raw_simd", config, || {
        processor.process_batch_raw_simd(data)
    });
    let optimized = bench_variant("optimized", config, || {
        processor.process_batch_optimized(data, schema)
    });
    let structural = bench_variant("structural", config, || {
        processor.process_batch_structural_filtering(data, schema)
    });

    // TODO: Implement CPU tape builder
    // let mut cpu_tape_builder = CpuTapeBuilder::new();
    // let tape_cpu = bench_tape_variant("structural_tape_cpu", config, || {
    //     let _ = cpu_tape_builder.build_structural_tape(data)?;
    //     Ok(())
    // });
    let tape_cpu = None;

    let gpu_tape_result = None; // TODO: Implement GPU tape builder

    print_result("baseline_serde", &baseline);
    print_result("raw_simd", &raw);
    print_result("optimized", &optimized);
    print_result("structural", &structural);
    if let Some(ref result) = tape_cpu {
        print_result("structural_tape_cpu", result);
    }
    if let Some(gpu_tape_result) = gpu_tape_result {
        print_result("structural_tape_gpu", &gpu_tape_result);
    }
    Ok(())
}

fn bench_variant<F>(name: &str, config: &Config, mut f: F) -> BenchResult
where
    F: FnMut() -> fionn_stream::skiptape::error::Result<fionn_stream::skiptape::jsonl::BatchResult>,
{
    for _ in 0..config.warmup {
        let _ = f();
    }

    let mut total = Duration::ZERO;
    for _ in 0..config.runs {
        let start = Instant::now();
        let _ = f().expect(name);
        total += start.elapsed();
    }

    BenchResult {
        _name: name.to_string(),
        avg: total / config.runs,
        runs: config.runs,
    }
}

#[allow(dead_code)]
fn bench_tape_variant<F>(name: &str, config: &Config, mut f: F) -> BenchResult
where
    F: FnMut() -> fionn_stream::skiptape::error::Result<()>,
{
    for _ in 0..config.warmup {
        let _ = f();
    }

    let mut total = Duration::ZERO;
    for _ in 0..config.runs {
        let start = Instant::now();
        f().expect(name);
        total += start.elapsed();
    }

    BenchResult {
        _name: name.to_string(),
        avg: total / config.runs,
        runs: config.runs,
    }
}

fn print_result(label: &str, result: &BenchResult) {
    println!("{label}: runs={}, avg={:?}", result.runs, result.avg);
}

#[allow(clippy::unnecessary_wraps)]
fn baseline_serde(
    data: &[u8],
) -> fionn_stream::skiptape::error::Result<fionn_stream::skiptape::jsonl::BatchResult> {
    let mut documents = Vec::new();
    let mut errors = Vec::new();
    let mut line_start = 0usize;

    for line_end in memchr_iter(b'\n', data).chain(std::iter::once(data.len())) {
        if line_end <= line_start {
            line_start = line_end + 1;
            continue;
        }
        let line = &data[line_start..line_end];
        line_start = line_end + 1;

        if line.is_empty() {
            continue;
        }

        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(value) => {
                documents.push(value.to_string());
            }
            Err(err) => {
                errors.push(fionn_stream::skiptape::jsonl::LineError {
                    line_index: errors.len(),
                    error: fionn_stream::skiptape::SkipTapeError::ParseError(err.to_string()),
                    raw_line: String::from_utf8_lossy(line).to_string(),
                });
            }
        }
    }

    Ok(fionn_stream::skiptape::jsonl::BatchResult {
        documents,
        errors,
        statistics: fionn_stream::skiptape::jsonl::BatchStatistics {
            total_lines: 0,
            successful_lines: 0,
            failed_lines: 0,
            processing_time_ms: 0.0,
            avg_memory_per_line: 0,
            overall_schema_match_ratio: 0.0,
        },
    })
}

struct BenchResult {
    _name: String,
    avg: Duration,
    runs: u32,
}

fn parse_bytes(input: &str) -> Option<usize> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut chars = trimmed.chars();
    let mut number_part = String::new();
    let mut suffix = None;
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            number_part.push(ch);
        } else {
            suffix = Some(ch);
            number_part.extend(chars);
            break;
        }
    }
    let number: usize = number_part.parse().ok()?;
    match suffix {
        None => Some(number),
        Some('k' | 'K') => Some(number * 1024),
        Some('m' | 'M') => Some(number * 1024 * 1024),
        Some('g' | 'G') => Some(number * 1024 * 1024 * 1024),
        _ => None,
    }
}

fn run_windows_driver(config: &Config) -> Result<(), String> {
    let exe_path = if let Some(path) = &config.windows_exe {
        path.clone()
    } else {
        default_windows_exe_path()?
    };

    let mut command_args = Vec::new();
    command_args.push(format!("--lines {}", config.lines));
    command_args.push(format!("--match-ratio {}", config.match_ratio));
    command_args.push(format!("--runs {}", config.runs));
    command_args.push(format!("--warmup {}", config.warmup));
    if config.cpu && !config.gpu {
        command_args.push("--cpu-only".to_string());
    } else if config.gpu && !config.cpu {
        command_args.push("--gpu-only".to_string());
    }
    command_args.push("--windows-only".to_string());

    let escaped = exe_path.replace('\'', "''");
    let joined_args = command_args.join(" ");
    let powershell_cmd = format!("& '{escaped}' {joined_args}");

    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &powershell_cmd])
        .output()
        .or_else(|_| {
            std::process::Command::new(
                "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
            )
            .args(["-NoProfile", "-Command", &powershell_cmd])
            .output()
        })
        .map_err(|err| format!("Failed to launch PowerShell: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "Windows exe failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    println!(
        "Windows exe output:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    if !output.stderr.is_empty() {
        eprintln!(
            "Windows exe stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn default_windows_exe_path() -> Result<String, String> {
    let distro = std::env::var("WSL_DISTRO_NAME").unwrap_or_else(|_| "Ubuntu".to_string());
    let cwd = std::env::current_dir().map_err(|err| format!("cwd error: {err}"))?;
    let mut windows_path = String::from(r"\\wsl$\");
    windows_path.push_str(&distro);
    windows_path.push('\\');
    windows_path.push_str(
        cwd.to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches('\\'),
    );
    windows_path.push_str("\\target\\release\\gpu_jsonl_runner.exe");
    Ok(windows_path)
}
