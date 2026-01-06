// SPDX-License-Identifier: MIT OR Apache-2.0
use dashmap::DashMap;
use memchr::{memchr, memchr_iter, memchr2};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

/// Component of a JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathComponent {
    /// A field name (e.g., "name" in "user.name")
    Field(String),
    /// An array index (e.g., 0 in "items\[0\]")
    ArrayIndex(usize),
}

/// Borrowed component of a JSON path.
#[derive(Debug, Clone, Copy)]
pub enum PathComponentRef<'a> {
    /// A field name reference
    Field(&'a str),
    /// An array index
    ArrayIndex(usize),
}

/// Stored path component referencing the owned path string.
#[derive(Debug, Clone)]
pub enum PathComponentRange {
    /// A field name as a range into the path string
    Field(Range<usize>),
    /// An array index
    ArrayIndex(usize),
}

/// Parsed path with owned storage and component ranges.
#[derive(Debug, Clone)]
pub struct ParsedPath {
    path: String,
    components: Vec<PathComponentRange>,
}

impl ParsedPath {
    /// Parse a path string into components.
    #[must_use]
    pub fn parse(path: &str) -> Self {
        let mut components = Vec::new();
        parse_simd_ranges(path, &mut components);
        Self {
            path: path.to_string(),
            components,
        }
    }

    /// Get the original path string.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the parsed components.
    #[must_use]
    pub fn components(&self) -> &[PathComponentRange] {
        &self.components
    }

    /// Convert components to borrowed references.
    pub fn components_ref<'a>(&'a self, out: &mut Vec<PathComponentRef<'a>>) {
        out.clear();
        out.reserve(self.components.len());
        for component in &self.components {
            match component {
                PathComponentRange::Field(range) => {
                    out.push(PathComponentRef::Field(&self.path[range.clone()]));
                }
                PathComponentRange::ArrayIndex(index) => {
                    out.push(PathComponentRef::ArrayIndex(*index));
                }
            }
        }
    }
}

/// Concurrent cache for parsed paths.
pub struct PathCache {
    map: DashMap<String, Arc<ParsedPath>>,
}

impl PathCache {
    /// Create a new empty path cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Get a cached parsed path or parse and cache it.
    #[must_use]
    pub fn get_or_parse(&self, path: &str) -> Arc<ParsedPath> {
        if let Some(entry) = self.map.get(path) {
            return entry.clone();
        }

        let parsed = Arc::new(ParsedPath::parse(path));
        self.map.insert(path.to_string(), parsed.clone());
        parsed
    }
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new()
    }
}

const SIMD_CUTOFF_DEFAULT: usize = 64;
const SIMD_CUTOFF_64: usize = 64;
const SIMD_CUTOFF_96: usize = 96;
const SIMD_CUTOFF_128: usize = 128;

// x86/x86_64 SIMD thresholds
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SIMD_SSE2_THRESHOLD: usize = 64;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SIMD_AVX2_THRESHOLD: usize = 128;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SIMD_AVX512_THRESHOLD: usize = 256;

/// Baseline scalar JSON path parser.
#[inline]
#[must_use]
pub fn parse_baseline(path: &str) -> Vec<PathComponent> {
    let mut components = Vec::new();
    let bytes = path.as_bytes();
    components.reserve(estimate_components(bytes));
    let mut current_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        let next = memchr2(b'.', b'[', &bytes[i..]);
        let Some(rel_pos) = next else {
            break;
        };
        let pos = i + rel_pos;

        match bytes[pos] {
            b'.' => {
                if pos > current_start {
                    let field_name = extract_field_name(bytes, current_start, pos);
                    components.push(PathComponent::Field(field_name));
                }
                current_start = pos + 1;
                i = pos + 1;
            }
            b'[' => {
                if pos > current_start {
                    let field_name = extract_field_name(bytes, current_start, pos);
                    components.push(PathComponent::Field(field_name));
                }

                let start = pos + 1;
                let end_rel = memchr(b']', &bytes[start..]);
                if let Some(rel_end) = end_rel {
                    let end = start + rel_end;
                    let index_str = &bytes[start..end];
                    let index = parse_usize(index_str);
                    components.push(PathComponent::ArrayIndex(index));
                    current_start = end + 1;
                    i = end + 1;
                } else {
                    break;
                }
            }
            _ => {
                i = pos + 1;
            }
        }
    }

    if current_start < bytes.len() {
        let field_name = extract_field_name(bytes, current_start, bytes.len());
        components.push(PathComponent::Field(field_name));
    }

    components
}

/// Original scalar JSON path parser (byte iteration).
#[inline]
#[must_use]
pub fn parse_original(path: &str) -> Vec<PathComponent> {
    let mut components = Vec::new();
    let mut current_start = 0;
    let bytes = path.as_bytes();
    components.reserve(estimate_components(bytes));

    for (i, &byte) in bytes.iter().enumerate() {
        match byte {
            b'.' => {
                if i > current_start {
                    let field_name = extract_field_name(bytes, current_start, i);
                    components.push(PathComponent::Field(field_name));
                }
                current_start = i + 1;
            }
            b'[' => {
                if i > current_start {
                    let field_name = extract_field_name(bytes, current_start, i);
                    components.push(PathComponent::Field(field_name));
                }

                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b']' {
                    end += 1;
                }

                if end < bytes.len() {
                    let index_str = &bytes[start..end];
                    let index = parse_usize(index_str);
                    components.push(PathComponent::ArrayIndex(index));
                    current_start = end + 1;
                }
            }
            _ => {}
        }
    }

    if current_start < bytes.len() {
        let field_name = extract_field_name(bytes, current_start, bytes.len());
        components.push(PathComponent::Field(field_name));
    }

    components
}

/// SIMD-friendly JSON path parser with explicit SIMD delimiter scan.
#[inline]
#[must_use]
pub fn parse_simd(path: &str) -> Vec<PathComponent> {
    parse_simd_with_cutoff(path, SIMD_CUTOFF_DEFAULT)
}

/// Parse path with 64-byte SIMD cutoff.
#[inline]
#[must_use]
pub fn parse_simd_cutoff_64(path: &str) -> Vec<PathComponent> {
    parse_simd_with_cutoff(path, SIMD_CUTOFF_64)
}

/// Parse path with 96-byte SIMD cutoff.
#[inline]
#[must_use]
pub fn parse_simd_cutoff_96(path: &str) -> Vec<PathComponent> {
    parse_simd_with_cutoff(path, SIMD_CUTOFF_96)
}

/// Parse path with 128-byte SIMD cutoff.
#[inline]
#[must_use]
pub fn parse_simd_cutoff_128(path: &str) -> Vec<PathComponent> {
    parse_simd_with_cutoff(path, SIMD_CUTOFF_128)
}

/// Parse path returning borrowed component references.
#[inline]
#[must_use]
pub fn parse_simd_ref(path: &str) -> Vec<PathComponentRef<'_>> {
    let mut components = Vec::new();
    parse_simd_ref_into(path, &mut components);
    components
}

/// Parse path into borrowed components using provided buffer.
#[inline]
pub fn parse_simd_ref_into<'a>(path: &'a str, components: &mut Vec<PathComponentRef<'a>>) {
    components.clear();
    let bytes = path.as_bytes();
    let mut current_start = 0;
    let mut i = 0;
    components.reserve(estimate_components(bytes));
    let dispatch = DISPATCH.get_or_init(init_dispatch);

    while i < bytes.len() {
        let remaining = bytes.len().saturating_sub(i);
        let Some(pos) = (if remaining < SIMD_CUTOFF_DEFAULT {
            memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
        } else {
            find_delim_dynamic(bytes, i, remaining, *dispatch)
        }) else {
            break;
        };

        match bytes[pos] {
            b'.' => {
                if pos > current_start {
                    let field_name = extract_field_name_ref(bytes, current_start, pos);
                    components.push(PathComponentRef::Field(field_name));
                }
                current_start = pos + 1;
                i = pos + 1;
            }
            b'[' => {
                if pos > current_start {
                    let field_name = extract_field_name_ref(bytes, current_start, pos);
                    components.push(PathComponentRef::Field(field_name));
                }

                let start = pos + 1;
                let remaining = bytes.len().saturating_sub(start);
                let end = if remaining < SIMD_CUTOFF_DEFAULT {
                    memchr(b']', &bytes[start..]).map(|pos| start + pos)
                } else {
                    find_byte_dynamic(bytes, start, remaining, b']', *dispatch)
                };
                if let Some(end) = end {
                    let index_str = &bytes[start..end];
                    let index = parse_usize(index_str);
                    components.push(PathComponentRef::ArrayIndex(index));
                    current_start = end + 1;
                    i = end + 1;
                } else {
                    break;
                }
            }
            _ => {
                i = pos + 1;
            }
        }
    }

    if current_start < bytes.len() {
        let field_name = extract_field_name_ref(bytes, current_start, bytes.len());
        components.push(PathComponentRef::Field(field_name));
    }
}

#[inline]
fn parse_simd_ranges(path: &str, components: &mut Vec<PathComponentRange>) {
    components.clear();
    let bytes = path.as_bytes();
    let mut current_start = 0;
    let mut i = 0;
    components.reserve(estimate_components(bytes));
    let dispatch = DISPATCH.get_or_init(init_dispatch);

    while i < bytes.len() {
        let remaining = bytes.len().saturating_sub(i);
        let Some(pos) = (if remaining < SIMD_CUTOFF_DEFAULT {
            memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
        } else {
            find_delim_dynamic(bytes, i, remaining, *dispatch)
        }) else {
            break;
        };

        match bytes[pos] {
            b'.' => {
                if pos > current_start {
                    components.push(PathComponentRange::Field(current_start..pos));
                }
                current_start = pos + 1;
                i = pos + 1;
            }
            b'[' => {
                if pos > current_start {
                    components.push(PathComponentRange::Field(current_start..pos));
                }

                let start = pos + 1;
                let remaining = bytes.len().saturating_sub(start);
                let end = if remaining < SIMD_CUTOFF_DEFAULT {
                    memchr(b']', &bytes[start..]).map(|pos| start + pos)
                } else {
                    find_byte_dynamic(bytes, start, remaining, b']', *dispatch)
                };
                if let Some(end) = end {
                    let index_str = &bytes[start..end];
                    let index = parse_usize(index_str);
                    components.push(PathComponentRange::ArrayIndex(index));
                    current_start = end + 1;
                    i = end + 1;
                } else {
                    break;
                }
            }
            _ => {
                i = pos + 1;
            }
        }
    }

    if current_start < bytes.len() {
        components.push(PathComponentRange::Field(current_start..bytes.len()));
    }
}

/// Parse path using forced AVX2 instructions.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn parse_simd_forced_avx2(path: &str) -> Vec<PathComponent> {
    parse_simd_with_forced(path, find_delim_avx2_wrapper, find_byte_avx2_wrapper)
}

/// Parse path using forced AVX-512 instructions.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn parse_simd_forced_avx512(path: &str) -> Vec<PathComponent> {
    parse_simd_with_forced(path, find_delim_avx512_wrapper, find_byte_avx512_wrapper)
}

/// Parse path using forced SSE2 instructions.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn parse_simd_forced_sse2(path: &str) -> Vec<PathComponent> {
    parse_simd_with_forced(path, find_delim_sse2, find_byte_sse2)
}

/// Parse path using forced SSE2 instructions.
#[cfg(target_arch = "x86")]
#[inline]
pub fn parse_simd_forced_sse2(path: &str) -> Vec<PathComponent> {
    parse_simd_with_forced(path, find_delim_sse2, find_byte_sse2)
}

#[inline]
fn parse_simd_with_cutoff(path: &str, cutoff: usize) -> Vec<PathComponent> {
    parse_simd_with(path, cutoff, None)
}

/// Helper for x86 forced SIMD instruction parsing
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
fn parse_simd_with_forced(
    path: &str,
    find_delim: DelimFinder,
    find_byte: ByteFinder,
) -> Vec<PathComponent> {
    parse_simd_with(path, 0, Some((find_delim, find_byte)))
}

#[inline]
fn parse_simd_with(
    path: &str,
    cutoff: usize,
    forced: Option<(DelimFinder, ByteFinder)>,
) -> Vec<PathComponent> {
    let mut components = Vec::new();
    let bytes = path.as_bytes();
    let mut current_start = 0;
    let mut i = 0;
    components.reserve(estimate_components(bytes));
    let dispatch = DISPATCH.get_or_init(init_dispatch);

    while i < bytes.len() {
        let remaining = bytes.len().saturating_sub(i);
        let Some(pos) = (if remaining < cutoff {
            memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
        } else if let Some((find_delim, _)) = forced {
            find_delim(bytes, i)
        } else {
            find_delim_dynamic(bytes, i, remaining, *dispatch)
        }) else {
            break;
        };

        match bytes[pos] {
            b'.' => {
                if pos > current_start {
                    let field_name = extract_field_name(bytes, current_start, pos);
                    components.push(PathComponent::Field(field_name));
                }
                current_start = pos + 1;
                i = pos + 1;
            }
            b'[' => {
                if pos > current_start {
                    let field_name = extract_field_name(bytes, current_start, pos);
                    components.push(PathComponent::Field(field_name));
                }

                let start = pos + 1;
                let remaining = bytes.len().saturating_sub(start);
                let end = if remaining < cutoff {
                    memchr(b']', &bytes[start..]).map(|pos| start + pos)
                } else if let Some((_, find_byte)) = forced {
                    find_byte(bytes, start, b']')
                } else {
                    find_byte_dynamic(bytes, start, remaining, b']', *dispatch)
                };
                if let Some(end) = end {
                    let index_str = &bytes[start..end];
                    let index = parse_usize(index_str);
                    components.push(PathComponent::ArrayIndex(index));
                    current_start = end + 1;
                    i = end + 1;
                } else {
                    break;
                }
            }
            _ => {
                i = pos + 1;
            }
        }
    }

    if current_start < bytes.len() {
        let field_name = extract_field_name(bytes, current_start, bytes.len());
        components.push(PathComponent::Field(field_name));
    }

    components
}

/// Extract field name as owned String from byte slice.
///
/// # Safety
/// Caller must ensure `bytes[start..end]` contains valid UTF-8.
/// This is guaranteed when `bytes` originates from `&str::as_bytes()`.
#[inline]
fn extract_field_name(bytes: &[u8], start: usize, end: usize) -> String {
    // SAFETY: All callers pass bytes from `&str::as_bytes()`, which guarantees valid UTF-8.
    // The start..end range is computed by scanning for ASCII delimiters (`.` and `[`),
    // which cannot split a multi-byte UTF-8 sequence.
    unsafe { String::from_utf8_unchecked(bytes[start..end].to_vec()) }
}

/// Extract field name as borrowed str from byte slice.
///
/// # Safety
/// Caller must ensure `bytes[start..end]` contains valid UTF-8.
/// This is guaranteed when `bytes` originates from `&str::as_bytes()`.
#[inline]
fn extract_field_name_ref(bytes: &[u8], start: usize, end: usize) -> &str {
    // SAFETY: All callers pass bytes from `&str::as_bytes()`, which guarantees valid UTF-8.
    // The start..end range is computed by scanning for ASCII delimiters (`.` and `[`),
    // which cannot split a multi-byte UTF-8 sequence.
    unsafe { std::str::from_utf8_unchecked(&bytes[start..end]) }
}

#[inline]
fn parse_usize(bytes: &[u8]) -> usize {
    let mut result = 0usize;
    for &byte in bytes {
        if byte.is_ascii_digit() {
            // Use saturating arithmetic to prevent overflow on huge digit strings
            result = result
                .saturating_mul(10)
                .saturating_add((byte - b'0') as usize);
        }
    }
    result
}

#[inline]
fn estimate_components(bytes: &[u8]) -> usize {
    let dots = memchr_iter(b'.', bytes).count();
    let brackets = memchr_iter(b'[', bytes).count();
    dots + brackets + 1
}

type DelimFinder = fn(&[u8], usize) -> Option<usize>;
type ByteFinder = fn(&[u8], usize, u8) -> Option<usize>;

#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // These are CPU feature flags, bools are appropriate
#[allow(dead_code)] // Fields used conditionally based on target_arch
struct SimdDispatch {
    has_sse2: bool,
    has_avx2: bool,
    has_avx512: bool,
    has_neon: bool,
}

static DISPATCH: OnceLock<SimdDispatch> = OnceLock::new();

#[cfg(target_arch = "x86_64")]
fn init_dispatch() -> SimdDispatch {
    SimdDispatch {
        has_sse2: std::is_x86_feature_detected!("sse2"),
        has_avx2: std::is_x86_feature_detected!("avx2"),
        has_avx512: std::is_x86_feature_detected!("avx512bw")
            && std::is_x86_feature_detected!("avx512f"),
        has_neon: false,
    }
}

#[cfg(target_arch = "x86")]
fn init_dispatch() -> SimdDispatch {
    SimdDispatch {
        has_sse2: std::is_x86_feature_detected!("sse2"),
        has_avx2: false,
        has_avx512: false,
        has_neon: false,
    }
}

#[cfg(target_arch = "aarch64")]
const fn init_dispatch() -> SimdDispatch {
    SimdDispatch {
        has_sse2: false,
        has_avx2: false,
        has_avx512: false,
        has_neon: true, // NEON is mandatory on aarch64
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn init_dispatch() -> SimdDispatch {
    SimdDispatch {
        has_sse2: false,
        has_avx2: false,
        has_avx512: false,
        has_neon: false,
    }
}

/// NEON threshold for using SIMD path finding (16 bytes = 128-bit vector)
#[cfg(target_arch = "aarch64")]
const SIMD_NEON_THRESHOLD: usize = 16;

#[inline]
fn find_delim_dynamic(
    bytes: &[u8],
    start: usize,
    remaining: usize,
    dispatch: SimdDispatch,
) -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        if dispatch.has_avx512 && remaining >= SIMD_AVX512_THRESHOLD {
            return find_delim_avx512_wrapper(bytes, start);
        }
        if dispatch.has_avx2 && remaining >= SIMD_AVX2_THRESHOLD {
            return find_delim_avx2_wrapper(bytes, start);
        }
        if dispatch.has_sse2 && remaining >= SIMD_SSE2_THRESHOLD {
            return find_delim_sse2(bytes, start);
        }
    }

    #[cfg(target_arch = "x86")]
    {
        if dispatch.has_sse2 && remaining >= SIMD_SSE2_THRESHOLD {
            return find_delim_sse2(bytes, start);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if dispatch.has_neon && remaining >= SIMD_NEON_THRESHOLD {
            return find_delim_neon(bytes, start);
        }
    }

    memchr2(b'.', b'[', &bytes[start..]).map(|pos| start + pos)
}

#[inline]
fn find_byte_dynamic(
    bytes: &[u8],
    start: usize,
    remaining: usize,
    needle: u8,
    dispatch: SimdDispatch,
) -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        if dispatch.has_avx512 && remaining >= SIMD_AVX512_THRESHOLD {
            return find_byte_avx512_wrapper(bytes, start, needle);
        }
        if dispatch.has_avx2 && remaining >= SIMD_AVX2_THRESHOLD {
            return find_byte_avx2_wrapper(bytes, start, needle);
        }
        if dispatch.has_sse2 && remaining >= SIMD_SSE2_THRESHOLD {
            return find_byte_sse2(bytes, start, needle);
        }
    }

    #[cfg(target_arch = "x86")]
    {
        if dispatch.has_sse2 && remaining >= SIMD_SSE2_THRESHOLD {
            return find_byte_sse2(bytes, start, needle);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if dispatch.has_neon && remaining >= SIMD_NEON_THRESHOLD {
            return find_byte_neon(bytes, start, needle);
        }
    }

    memchr(needle, &bytes[start..]).map(|pos| start + pos)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_delim_sse2(bytes: &[u8], start: usize) -> Option<usize> {
    use std::arch::x86_64::{
        _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };

    let mut i = start;
    let len = bytes.len();

    let dot = unsafe { _mm_set1_epi8(b'.'.cast_signed()) };
    let bracket = unsafe { _mm_set1_epi8(b'['.cast_signed()) };

    while i + 16 <= len {
        let chunk = unsafe { _mm_loadu_si128(bytes.as_ptr().add(i).cast()) };
        let eq_dot = unsafe { _mm_cmpeq_epi8(chunk, dot) };
        let eq_bracket = unsafe { _mm_cmpeq_epi8(chunk, bracket) };
        let mask = unsafe { _mm_movemask_epi8(_mm_or_si128(eq_dot, eq_bracket)) }.cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 16;
    }

    memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn find_delim_avx2(bytes: &[u8], start: usize) -> Option<usize> {
    use std::arch::x86_64::{
        _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_or_si256,
        _mm256_set1_epi8,
    };

    let mut i = start;
    let len = bytes.len();
    let dot = _mm256_set1_epi8(b'.'.cast_signed());
    let bracket = _mm256_set1_epi8(b'['.cast_signed());

    while i + 32 <= len {
        let chunk = unsafe { _mm256_loadu_si256(bytes.as_ptr().add(i).cast()) };
        let eq_dot = _mm256_cmpeq_epi8(chunk, dot);
        let eq_bracket = _mm256_cmpeq_epi8(chunk, bracket);
        let mask = _mm256_movemask_epi8(_mm256_or_si256(eq_dot, eq_bracket)).cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 32;
    }

    memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_delim_avx2_wrapper(bytes: &[u8], start: usize) -> Option<usize> {
    unsafe { find_delim_avx2(bytes, start) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512bw,avx512f")]
#[inline]
unsafe fn find_delim_avx512(bytes: &[u8], start: usize) -> Option<usize> {
    use std::arch::x86_64::{_mm512_cmpeq_epi8_mask, _mm512_loadu_si512, _mm512_set1_epi8};

    let mut i = start;
    let len = bytes.len();
    let dot = _mm512_set1_epi8(b'.'.cast_signed());
    let bracket = _mm512_set1_epi8(b'['.cast_signed());

    while i + 64 <= len {
        let chunk = unsafe { _mm512_loadu_si512(bytes.as_ptr().add(i).cast()) };
        let dot_mask = _mm512_cmpeq_epi8_mask(chunk, dot);
        let bracket_mask = _mm512_cmpeq_epi8_mask(chunk, bracket);
        let mask = dot_mask | bracket_mask;

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 64;
    }

    memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_delim_avx512_wrapper(bytes: &[u8], start: usize) -> Option<usize> {
    unsafe { find_delim_avx512(bytes, start) }
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_byte_sse2(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    use std::arch::x86_64::{_mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8};

    let mut i = start;
    let len = bytes.len();
    let needle_vec = unsafe { _mm_set1_epi8(needle.cast_signed()) };

    while i + 16 <= len {
        let chunk = unsafe { _mm_loadu_si128(bytes.as_ptr().add(i).cast()) };
        let eq = unsafe { _mm_cmpeq_epi8(chunk, needle_vec) };
        let mask = unsafe { _mm_movemask_epi8(eq) }.cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 16;
    }

    memchr(needle, &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn find_byte_avx2(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    use std::arch::x86_64::{
        _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_set1_epi8,
    };

    let mut i = start;
    let len = bytes.len();
    let needle_vec = _mm256_set1_epi8(needle.cast_signed());

    while i + 32 <= len {
        let chunk = unsafe { _mm256_loadu_si256(bytes.as_ptr().add(i).cast()) };
        let eq = _mm256_cmpeq_epi8(chunk, needle_vec);
        let mask = _mm256_movemask_epi8(eq).cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 32;
    }

    memchr(needle, &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_byte_avx2_wrapper(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    unsafe { find_byte_avx2(bytes, start, needle) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512bw,avx512f")]
#[inline]
unsafe fn find_byte_avx512(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    use std::arch::x86_64::{_mm512_cmpeq_epi8_mask, _mm512_loadu_si512, _mm512_set1_epi8};

    let mut i = start;
    let len = bytes.len();
    let needle_vec = _mm512_set1_epi8(needle.cast_signed());

    while i + 64 <= len {
        let chunk = unsafe { _mm512_loadu_si512(bytes.as_ptr().add(i).cast()) };
        let mask = _mm512_cmpeq_epi8_mask(chunk, needle_vec);

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 64;
    }

    memchr(needle, &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn find_byte_avx512_wrapper(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    unsafe { find_byte_avx512(bytes, start, needle) }
}

#[cfg(target_arch = "x86")]
#[inline]
fn find_delim_sse2(bytes: &[u8], start: usize) -> Option<usize> {
    use std::arch::x86::{
        _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };

    let mut i = start;
    let len = bytes.len();

    let dot = unsafe { _mm_set1_epi8(b'.'.cast_signed()) };
    let bracket = unsafe { _mm_set1_epi8(b'['.cast_signed()) };

    while i + 16 <= len {
        let chunk = unsafe { _mm_loadu_si128(bytes.as_ptr().add(i).cast()) };
        let eq_dot = unsafe { _mm_cmpeq_epi8(chunk, dot) };
        let eq_bracket = unsafe { _mm_cmpeq_epi8(chunk, bracket) };
        let mask = unsafe { _mm_movemask_epi8(_mm_or_si128(eq_dot, eq_bracket)) }.cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 16;
    }

    memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
}

#[cfg(target_arch = "x86")]
#[inline]
fn find_byte_sse2(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    use std::arch::x86::{_mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_set1_epi8};

    let mut i = start;
    let len = bytes.len();
    let needle_vec = unsafe { _mm_set1_epi8(needle.cast_signed()) };

    while i + 16 <= len {
        let chunk = unsafe { _mm_loadu_si128(bytes.as_ptr().add(i).cast()) };
        let eq = unsafe { _mm_cmpeq_epi8(chunk, needle_vec) };
        let mask = unsafe { _mm_movemask_epi8(eq) }.cast_unsigned();

        if mask != 0 {
            let offset = mask.trailing_zeros() as usize;
            return Some(i + offset);
        }

        i += 16;
    }

    memchr(needle, &bytes[i..]).map(|pos| i + pos)
}

// =============================================================================
// ARM NEON implementations
// =============================================================================

/// Find delimiter (`.` or `[`) using ARM NEON SIMD
#[cfg(target_arch = "aarch64")]
#[inline]
fn find_delim_neon(bytes: &[u8], start: usize) -> Option<usize> {
    use std::arch::aarch64::{vceqq_u8, vdupq_n_u8, vld1q_u8, vorrq_u8};

    let mut i = start;
    let len = bytes.len();

    let dot = unsafe { vdupq_n_u8(b'.') };
    let bracket = unsafe { vdupq_n_u8(b'[') };

    while i + 16 <= len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(i)) };
        let eq_dot = unsafe { vceqq_u8(chunk, dot) };
        let eq_bracket = unsafe { vceqq_u8(chunk, bracket) };
        let combined = unsafe { vorrq_u8(eq_dot, eq_bracket) };

        // SAFETY: combined is a valid NEON vector from the operations above
        if let Some(offset) = unsafe { neon_first_set_byte(combined) } {
            return Some(i + offset);
        }

        i += 16;
    }

    memchr2(b'.', b'[', &bytes[i..]).map(|pos| i + pos)
}

/// Find single byte using ARM NEON SIMD
#[cfg(target_arch = "aarch64")]
#[inline]
fn find_byte_neon(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    use std::arch::aarch64::{vceqq_u8, vdupq_n_u8, vld1q_u8};

    let mut i = start;
    let len = bytes.len();

    let needle_vec = unsafe { vdupq_n_u8(needle) };

    while i + 16 <= len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(i)) };
        let eq = unsafe { vceqq_u8(chunk, needle_vec) };

        // SAFETY: eq is a valid NEON vector from the operations above
        if let Some(offset) = unsafe { neon_first_set_byte(eq) } {
            return Some(i + offset);
        }

        i += 16;
    }

    memchr(needle, &bytes[i..]).map(|pos| i + pos)
}

/// Find the index of the first non-zero byte in a NEON vector
///
/// Returns None if all bytes are zero.
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_first_set_byte(v: std::arch::aarch64::uint8x16_t) -> Option<usize> {
    // SAFETY: uint8x16_t is a 16-byte SIMD vector, safe to transmute to [u8; 16]
    let arr: [u8; 16] = unsafe { std::mem::transmute(v) };
    for (i, &byte) in arr.iter().enumerate() {
        if byte != 0 {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // PathComponent tests
    #[test]
    fn test_path_component_field() {
        let comp = PathComponent::Field("test".to_string());
        if let PathComponent::Field(s) = comp {
            assert_eq!(s, "test");
        } else {
            panic!("Expected Field");
        }
    }

    #[test]
    fn test_path_component_array_index() {
        let comp = PathComponent::ArrayIndex(42);
        if let PathComponent::ArrayIndex(i) = comp {
            assert_eq!(i, 42);
        } else {
            panic!("Expected ArrayIndex");
        }
    }

    #[test]
    fn test_path_component_clone() {
        let comp = PathComponent::Field("test".to_string());
        let cloned = comp;
        if let PathComponent::Field(s) = cloned {
            assert_eq!(s, "test");
        }
    }

    #[test]
    fn test_path_component_debug() {
        let comp = PathComponent::Field("test".to_string());
        let debug = format!("{comp:?}");
        assert!(debug.contains("Field"));
    }

    // PathComponentRef tests
    #[test]
    fn test_path_component_ref_field() {
        let comp = PathComponentRef::Field("test");
        if let PathComponentRef::Field(s) = comp {
            assert_eq!(s, "test");
        } else {
            panic!("Expected Field");
        }
    }

    #[test]
    fn test_path_component_ref_array_index() {
        let comp = PathComponentRef::ArrayIndex(42);
        if let PathComponentRef::ArrayIndex(i) = comp {
            assert_eq!(i, 42);
        } else {
            panic!("Expected ArrayIndex");
        }
    }

    #[test]
    fn test_path_component_ref_copy() {
        let comp = PathComponentRef::Field("test");
        let copied = comp;
        if let PathComponentRef::Field(s) = copied {
            assert_eq!(s, "test");
        }
    }

    // PathComponentRange tests
    #[test]
    fn test_path_component_range_field() {
        let comp = PathComponentRange::Field(0..4);
        if let PathComponentRange::Field(range) = comp {
            assert_eq!(range, 0..4);
        } else {
            panic!("Expected Field");
        }
    }

    #[test]
    fn test_path_component_range_array_index() {
        let comp = PathComponentRange::ArrayIndex(42);
        if let PathComponentRange::ArrayIndex(i) = comp {
            assert_eq!(i, 42);
        } else {
            panic!("Expected ArrayIndex");
        }
    }

    #[test]
    fn test_path_component_range_clone() {
        let comp = PathComponentRange::Field(0..4);
        let cloned = comp;
        if let PathComponentRange::Field(range) = cloned {
            assert_eq!(range, 0..4);
        }
    }

    // ParsedPath tests
    #[test]
    fn test_parsed_path_simple() {
        let path = ParsedPath::parse("user.name");
        assert_eq!(path.path(), "user.name");
        assert_eq!(path.components().len(), 2);
    }

    #[test]
    fn test_parsed_path_with_array() {
        let path = ParsedPath::parse("users[0].name");
        assert_eq!(path.components().len(), 3);
    }

    #[test]
    fn test_parsed_path_empty() {
        let path = ParsedPath::parse("");
        assert_eq!(path.path(), "");
        assert_eq!(path.components().len(), 0);
    }

    #[test]
    fn test_parsed_path_single_field() {
        let path = ParsedPath::parse("field");
        assert_eq!(path.components().len(), 1);
    }

    #[test]
    fn test_parsed_path_components_ref() {
        let path = ParsedPath::parse("user.name");
        let mut refs = Vec::new();
        path.components_ref(&mut refs);
        assert_eq!(refs.len(), 2);
        if let PathComponentRef::Field(s) = refs[0] {
            assert_eq!(s, "user");
        }
        if let PathComponentRef::Field(s) = refs[1] {
            assert_eq!(s, "name");
        }
    }

    #[test]
    fn test_parsed_path_components_ref_with_array() {
        let path = ParsedPath::parse("users[5].name");
        let mut refs = Vec::new();
        path.components_ref(&mut refs);
        assert_eq!(refs.len(), 3);
        if let PathComponentRef::ArrayIndex(i) = refs[1] {
            assert_eq!(i, 5);
        }
    }

    #[test]
    fn test_parsed_path_clone() {
        let path = ParsedPath::parse("user.name");
        let cloned = path;
        assert_eq!(cloned.path(), "user.name");
    }

    // PathCache tests
    #[test]
    fn test_path_cache_new() {
        let cache = PathCache::new();
        let path = cache.get_or_parse("user.name");
        assert_eq!(path.path(), "user.name");
    }

    #[test]
    fn test_path_cache_returns_same_instance() {
        let cache = PathCache::new();
        let path1 = cache.get_or_parse("user.name");
        let path2 = cache.get_or_parse("user.name");
        assert!(Arc::ptr_eq(&path1, &path2));
    }

    #[test]
    fn test_path_cache_different_paths() {
        let cache = PathCache::new();
        let path1 = cache.get_or_parse("user.name");
        let path2 = cache.get_or_parse("user.age");
        assert!(!Arc::ptr_eq(&path1, &path2));
    }

    // parse_baseline tests
    #[test]
    fn test_parse_baseline_simple() {
        let components = parse_baseline("user.name");
        assert_eq!(components.len(), 2);
        if let PathComponent::Field(s) = &components[0] {
            assert_eq!(s, "user");
        }
    }

    #[test]
    fn test_parse_baseline_with_array() {
        let components = parse_baseline("users[0].name");
        assert_eq!(components.len(), 3);
        if let PathComponent::ArrayIndex(i) = &components[1] {
            assert_eq!(*i, 0);
        }
    }

    #[test]
    fn test_parse_baseline_empty() {
        let components = parse_baseline("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_baseline_single_field() {
        let components = parse_baseline("field");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_baseline_nested() {
        let components = parse_baseline("a.b.c.d.e");
        assert_eq!(components.len(), 5);
    }

    #[test]
    fn test_parse_baseline_multiple_arrays() {
        let components = parse_baseline("a[0][1][2]");
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn test_parse_baseline_leading_dot() {
        let components = parse_baseline(".field");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_baseline_trailing_dot() {
        let components = parse_baseline("field.");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_baseline_unclosed_bracket() {
        let components = parse_baseline("field[0");
        // Should handle gracefully
        assert!(!components.is_empty());
    }

    // parse_original tests
    #[test]
    fn test_parse_original_simple() {
        let components = parse_original("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_original_with_array() {
        let components = parse_original("users[0].name");
        assert_eq!(components.len(), 3);
    }

    #[test]
    fn test_parse_original_empty() {
        let components = parse_original("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_original_nested() {
        let components = parse_original("a.b.c.d.e");
        assert_eq!(components.len(), 5);
    }

    // parse_simd tests
    #[test]
    fn test_parse_simd_simple() {
        let components = parse_simd("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_with_array() {
        let components = parse_simd("users[0].name");
        assert_eq!(components.len(), 3);
    }

    #[test]
    fn test_parse_simd_empty() {
        let components = parse_simd("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_simd_nested() {
        let components = parse_simd("a.b.c.d.e");
        assert_eq!(components.len(), 5);
    }

    #[test]
    fn test_parse_simd_long_path() {
        // Create a path long enough to trigger SIMD processing
        let path = (0..100)
            .map(|i| format!("field{i}"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_simd(&path);
        assert_eq!(components.len(), 100);
    }

    // parse_simd cutoff variants
    #[test]
    fn test_parse_simd_cutoff_64() {
        let components = parse_simd_cutoff_64("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_cutoff_96() {
        let components = parse_simd_cutoff_96("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_cutoff_128() {
        let components = parse_simd_cutoff_128("user.name");
        assert_eq!(components.len(), 2);
    }

    // parse_simd_ref tests
    #[test]
    fn test_parse_simd_ref_simple() {
        let components = parse_simd_ref("user.name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_ref_with_array() {
        let components = parse_simd_ref("users[0].name");
        assert_eq!(components.len(), 3);
    }

    #[test]
    fn test_parse_simd_ref_empty() {
        let components = parse_simd_ref("");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_simd_ref_into() {
        let mut components = Vec::new();
        parse_simd_ref_into("user.name", &mut components);
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_ref_into_reuses_vec() {
        let mut components = Vec::new();
        parse_simd_ref_into("a.b.c", &mut components);
        assert_eq!(components.len(), 3);
        parse_simd_ref_into("x.y", &mut components);
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_simd_ref_long_path() {
        let path = (0..100)
            .map(|i| format!("field{i}"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_simd_ref(&path);
        assert_eq!(components.len(), 100);
    }

    // Forced SIMD variant tests (x86_64 only)
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx2() {
        if std::is_x86_feature_detected!("avx2") {
            let components = parse_simd_forced_avx2("user.name");
            assert_eq!(components.len(), 2);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx2_with_array() {
        if std::is_x86_feature_detected!("avx2") {
            let components = parse_simd_forced_avx2("users[0].name");
            assert_eq!(components.len(), 3);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx2_long() {
        if std::is_x86_feature_detected!("avx2") {
            let path = (0..100)
                .map(|i| format!("field{i}"))
                .collect::<Vec<_>>()
                .join(".");
            let components = parse_simd_forced_avx2(&path);
            assert_eq!(components.len(), 100);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx512() {
        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            let components = parse_simd_forced_avx512("user.name");
            assert_eq!(components.len(), 2);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx512_with_array() {
        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            let components = parse_simd_forced_avx512("users[0].name");
            assert_eq!(components.len(), 3);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_parse_simd_forced_avx512_long() {
        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            let path = (0..100)
                .map(|i| format!("field{i}"))
                .collect::<Vec<_>>()
                .join(".");
            let components = parse_simd_forced_avx512(&path);
            assert_eq!(components.len(), 100);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_parse_simd_forced_sse2() {
        if std::is_x86_feature_detected!("sse2") {
            let components = parse_simd_forced_sse2("user.name");
            assert_eq!(components.len(), 2);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_parse_simd_forced_sse2_with_array() {
        if std::is_x86_feature_detected!("sse2") {
            let components = parse_simd_forced_sse2("users[0].name");
            assert_eq!(components.len(), 3);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_parse_simd_forced_sse2_long() {
        if std::is_x86_feature_detected!("sse2") {
            let path = (0..100)
                .map(|i| format!("field{i}"))
                .collect::<Vec<_>>()
                .join(".");
            let components = parse_simd_forced_sse2(&path);
            assert_eq!(components.len(), 100);
        }
    }

    // Helper function tests
    #[test]
    fn test_parse_usize_basic() {
        assert_eq!(parse_usize(b"123"), 123);
        assert_eq!(parse_usize(b"0"), 0);
        assert_eq!(parse_usize(b"999"), 999);
    }

    #[test]
    fn test_parse_usize_empty() {
        assert_eq!(parse_usize(b""), 0);
    }

    #[test]
    fn test_parse_usize_with_non_digits() {
        assert_eq!(parse_usize(b"12x34"), 1234);
    }

    #[test]
    fn test_estimate_components() {
        // estimate_components counts dots + brackets + 1
        // "a.b.c" has 2 dots, 0 brackets => 2 + 0 + 1 = 3
        assert_eq!(estimate_components(b"a.b.c"), 3);
        // "a[0][1]" has 0 dots, 2 brackets => 0 + 2 + 1 = 3
        assert_eq!(estimate_components(b"a[0][1]"), 3);
        assert_eq!(estimate_components(b"field"), 1);
    }

    #[test]
    fn test_extract_field_name() {
        let bytes = b"hello.world";
        let field = extract_field_name(bytes, 0, 5);
        assert_eq!(field, "hello");
    }

    #[test]
    fn test_extract_field_name_ref() {
        let bytes = b"hello.world";
        let field = extract_field_name_ref(bytes, 0, 5);
        assert_eq!(field, "hello");
    }

    // SIMD dispatch tests
    #[test]
    fn test_simd_dispatch_init() {
        let dispatch = init_dispatch();
        // Just check that it doesn't panic
        let _ = dispatch.has_sse2;
        let _ = dispatch.has_avx2;
        let _ = dispatch.has_avx512;
    }

    // Edge case tests
    #[test]
    fn test_consecutive_dots() {
        let components = parse_simd("a..b");
        // Should skip empty field
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_consecutive_arrays() {
        let components = parse_simd("a[0][1][2]");
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn test_array_at_start() {
        let components = parse_simd("[0].name");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_array_at_end() {
        let components = parse_simd("users[0]");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_complex_path() {
        let components = parse_simd("data.users[0].profile.settings[1].value");
        // data, users, [0], profile, settings, [1], value = 7 components
        assert_eq!(components.len(), 7);
    }

    #[test]
    fn test_large_array_index() {
        let components = parse_simd("users[999999]");
        assert_eq!(components.len(), 2);
        if let PathComponent::ArrayIndex(i) = &components[1] {
            assert_eq!(*i, 999_999);
        }
    }

    // Comparison tests - ensure all parsers produce same results
    #[test]
    fn test_parsers_produce_same_results() {
        let paths = vec![
            "user.name",
            "users[0].name",
            "a.b.c.d.e",
            "data[0][1][2]",
            "simple",
            "",
        ];

        for path in paths {
            let baseline = parse_baseline(path);
            let original = parse_original(path);
            let simd = parse_simd(path);

            assert_eq!(baseline.len(), original.len(), "Path: {path}");
            assert_eq!(baseline.len(), simd.len(), "Path: {path}");

            for i in 0..baseline.len() {
                match (&baseline[i], &original[i], &simd[i]) {
                    (PathComponent::Field(a), PathComponent::Field(b), PathComponent::Field(c)) => {
                        assert_eq!(a, b);
                        assert_eq!(a, c);
                    }
                    (
                        PathComponent::ArrayIndex(a),
                        PathComponent::ArrayIndex(b),
                        PathComponent::ArrayIndex(c),
                    ) => {
                        assert_eq!(a, b);
                        assert_eq!(a, c);
                    }
                    _ => panic!("Mismatched component types at index {i} for path: {path}"),
                }
            }
        }
    }

    // Long path tests to trigger SIMD branches
    #[test]
    fn test_very_long_path_baseline() {
        let path = (0..200)
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_baseline(&path);
        assert_eq!(components.len(), 200);
    }

    #[test]
    fn test_very_long_path_simd() {
        let path = (0..200)
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_simd(&path);
        assert_eq!(components.len(), 200);
    }

    #[test]
    fn test_very_long_path_with_arrays() {
        let path = (0..50)
            .map(|i| format!("f{i}[{i}]"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_simd(&path);
        // Each segment has a field and an array index
        assert_eq!(components.len(), 100);
    }

    // Dynamic dispatch tests
    #[test]
    fn test_find_delim_dynamic_short_path() {
        let dispatch = init_dispatch();
        let bytes = b"a.b";
        let result = find_delim_dynamic(bytes, 0, bytes.len(), dispatch);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_find_delim_dynamic_no_delim() {
        let dispatch = init_dispatch();
        let bytes = b"abcdefgh";
        let result = find_delim_dynamic(bytes, 0, bytes.len(), dispatch);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_byte_dynamic_short() {
        let dispatch = init_dispatch();
        let bytes = b"a]b";
        let result = find_byte_dynamic(bytes, 0, bytes.len(), b']', dispatch);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_find_byte_dynamic_not_found() {
        let dispatch = init_dispatch();
        let bytes = b"abcdefgh";
        let result = find_byte_dynamic(bytes, 0, bytes.len(), b']', dispatch);
        assert_eq!(result, None);
    }

    // SSE2 tests (x86_64)
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_sse2_basic() {
        let bytes = b"abcdefghijklmnop.rest";
        let result = find_delim_sse2(bytes, 0);
        assert_eq!(result, Some(16));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_sse2_bracket() {
        let bytes = b"abcdefghijklmnop[rest";
        let result = find_delim_sse2(bytes, 0);
        assert_eq!(result, Some(16));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_sse2_no_match() {
        let bytes = b"abcdefghijklmnopqrstuvwxyz";
        let result = find_delim_sse2(bytes, 0);
        assert_eq!(result, None);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_byte_sse2_basic() {
        let bytes = b"abcdefghijklmnop]rest";
        let result = find_byte_sse2(bytes, 0, b']');
        assert_eq!(result, Some(16));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_byte_sse2_no_match() {
        let bytes = b"abcdefghijklmnopqrstuvwxyz";
        let result = find_byte_sse2(bytes, 0, b']');
        assert_eq!(result, None);
    }

    // AVX2 wrapper tests
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_avx2_wrapper() {
        if std::is_x86_feature_detected!("avx2") {
            let bytes = vec![b'x'; 100];
            let result = find_delim_avx2_wrapper(&bytes, 0);
            assert_eq!(result, None);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_byte_avx2_wrapper() {
        if std::is_x86_feature_detected!("avx2") {
            let bytes = vec![b'x'; 100];
            let result = find_byte_avx2_wrapper(&bytes, 0, b']');
            assert_eq!(result, None);
        }
    }

    // AVX512 wrapper tests
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_avx512_wrapper() {
        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            let bytes = vec![b'x'; 200];
            let result = find_delim_avx512_wrapper(&bytes, 0);
            assert_eq!(result, None);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_byte_avx512_wrapper() {
        if std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512f") {
            let bytes = vec![b'x'; 200];
            let result = find_byte_avx512_wrapper(&bytes, 0, b']');
            assert_eq!(result, None);
        }
    }

    // =========================================================================
    // Additional Coverage Tests
    // =========================================================================

    #[test]
    fn test_path_cache_default() {
        let cache = PathCache::default();
        let path = cache.get_or_parse("test");
        assert_eq!(path.path(), "test");
    }

    #[test]
    fn test_path_component_equality() {
        let a = PathComponent::Field("test".to_string());
        let b = PathComponent::Field("test".to_string());
        let c = PathComponent::Field("other".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_path_component_array_equality() {
        let a = PathComponent::ArrayIndex(0);
        let b = PathComponent::ArrayIndex(0);
        let c = PathComponent::ArrayIndex(1);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_path_component_mixed_inequality() {
        let field = PathComponent::Field("0".to_string());
        let index = PathComponent::ArrayIndex(0);
        assert_ne!(field, index);
    }

    #[test]
    fn test_path_component_ref_debug() {
        let comp = PathComponentRef::Field("test");
        let debug = format!("{comp:?}");
        assert!(debug.contains("Field"));
    }

    #[test]
    fn test_path_component_range_debug() {
        let comp = PathComponentRange::Field(0..4);
        let debug = format!("{comp:?}");
        assert!(debug.contains("Field"));
    }

    #[test]
    fn test_parse_simd_ranges_direct() {
        let mut components = Vec::new();
        parse_simd_ranges("user.name[0]", &mut components);
        assert_eq!(components.len(), 3);
    }

    #[test]
    fn test_parse_original_unclosed_bracket() {
        let components = parse_original("field[0");
        // Should handle gracefully
        assert!(!components.is_empty());
    }

    #[test]
    fn test_parse_original_leading_dot() {
        let components = parse_original(".field");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_original_trailing_dot() {
        let components = parse_original("field.");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_original_consecutive_dots() {
        let components = parse_original("a..b");
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_usize_large() {
        assert_eq!(parse_usize(b"1_234_567_890"), 1_234_567_890);
    }

    #[test]
    fn test_estimate_components_mixed() {
        // "a.b[0].c[1]" has 2 dots, 2 brackets => 2 + 2 + 1 = 5
        assert_eq!(estimate_components(b"a.b[0].c[1]"), 5);
    }

    #[test]
    fn test_extract_field_name_utf8() {
        let bytes = "日本語.field".as_bytes();
        let field = extract_field_name(bytes, 0, 9);
        assert_eq!(field, "日本語");
    }

    #[test]
    fn test_parsed_path_clone_trait() {
        let path = ParsedPath::parse("a.b");
        let cloned = path;
        assert_eq!(cloned.path(), "a.b");
    }

    #[test]
    fn test_parsed_path_debug() {
        let path = ParsedPath::parse("a.b");
        let debug = format!("{path:?}");
        assert!(debug.contains("ParsedPath"));
    }

    #[test]
    fn test_path_builder_many_indices() {
        let components = parse_simd("arr[0][1][2][3][4][5][6][7][8][9]");
        assert_eq!(components.len(), 11);
    }

    #[test]
    fn test_find_delim_dynamic_with_start() {
        let dispatch = init_dispatch();
        let bytes = b"aaa.bbb";
        let result = find_delim_dynamic(bytes, 2, bytes.len(), dispatch);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_find_byte_dynamic_with_start() {
        let dispatch = init_dispatch();
        let bytes = b"aa]bb]cc";
        let result = find_byte_dynamic(bytes, 3, bytes.len(), b']', dispatch);
        assert_eq!(result, Some(5));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_delim_sse2_with_offset() {
        let bytes = b"abcdefghijklmnop.qrstuvwxyz.123";
        let result = find_delim_sse2(bytes, 17);
        assert_eq!(result, Some(27));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_find_byte_sse2_with_offset() {
        let bytes = b"abcdefghijklmnop]qrstuvwxyz]123";
        let result = find_byte_sse2(bytes, 17, b']');
        assert_eq!(result, Some(27));
    }

    #[test]
    fn test_parse_simd_ref_long_with_arrays() {
        let path = (0..30)
            .map(|i| format!("field{i}[{i}]"))
            .collect::<Vec<_>>()
            .join(".");
        let components = parse_simd_ref(&path);
        assert_eq!(components.len(), 60);
    }

    #[test]
    fn test_simd_dispatch_fields() {
        let dispatch = init_dispatch();
        // Test all fields are accessible
        let _ = dispatch.has_sse2;
        let _ = dispatch.has_avx2;
        let _ = dispatch.has_avx512;
        let _ = dispatch.has_neon;
    }

    #[test]
    fn test_global_dispatch() {
        let dispatch = DISPATCH.get_or_init(init_dispatch);
        // Just verify it doesn't panic and returns valid dispatch
        let _ = dispatch.has_sse2;
    }

    #[test]
    fn test_parse_simd_very_short() {
        let components = parse_simd("a");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_baseline_only_bracket() {
        let components = parse_baseline("[0]");
        assert_eq!(components.len(), 1);
    }

    #[test]
    fn test_parse_simd_ref_only_bracket() {
        let components = parse_simd_ref("[0]");
        assert_eq!(components.len(), 1);
    }

    // Regression test for libFuzzer crash: stack overflow on huge array index
    // Found by: cargo +nightly fuzz run fuzz_tape_libfuzzer
    // Input: [2777777777777777777777777777777777\t\0\0\0\0\0\0\0]
    #[test]
    fn test_parse_usize_overflow_regression() {
        // This input caused stack overflow before saturating arithmetic fix
        let input = "[2777777777777777777777777777777777\t\0\0\0\0\0\0\0]";
        let components = parse_simd(input);
        // Should parse without crashing; index saturates to usize::MAX
        assert_eq!(components.len(), 1);
        match &components[0] {
            PathComponent::ArrayIndex(idx) => {
                // Saturated value - exact value doesn't matter, just shouldn't crash
                assert!(*idx > 0);
            }
            PathComponent::Field(_) => panic!("expected ArrayIndex"),
        }
    }

    #[test]
    fn test_parse_usize_large_number() {
        // Test that large numbers saturate instead of overflowing
        let input = "data[99999999999999999999999999999999]";
        let components = parse_simd(input);
        assert_eq!(components.len(), 2);
        match &components[1] {
            PathComponent::ArrayIndex(idx) => {
                assert_eq!(*idx, usize::MAX);
            }
            PathComponent::Field(_) => panic!("expected ArrayIndex"),
        }
    }
}
