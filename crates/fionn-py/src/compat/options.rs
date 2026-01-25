// SPDX-License-Identifier: MIT OR Apache-2.0
//! orjson-compatible option flags
//!
//! All 14 OPT_* flags from orjson 3.x, with identical semantics.

/// Append newline to output
pub const OPT_APPEND_NEWLINE: u32 = 1 << 0;

/// Pretty-print with 2-space indentation
pub const OPT_INDENT_2: u32 = 1 << 1;

/// Serialize naive datetime as UTC
pub const OPT_NAIVE_UTC: u32 = 1 << 2;

/// Allow non-string dict keys (serialize to JSON strings)
pub const OPT_NON_STR_KEYS: u32 = 1 << 3;

/// Omit microseconds from datetime serialization
pub const OPT_OMIT_MICROSECONDS: u32 = 1 << 4;

/// Pass dataclass instances to `default` function
pub const OPT_PASSTHROUGH_DATACLASS: u32 = 1 << 5;

/// Pass datetime instances to `default` function
pub const OPT_PASSTHROUGH_DATETIME: u32 = 1 << 6;

/// Pass subclass instances to `default` function
pub const OPT_PASSTHROUGH_SUBCLASS: u32 = 1 << 7;

/// Serialize dataclass instances (default behavior)
pub const OPT_SERIALIZE_DATACLASS: u32 = 1 << 8;

/// Serialize numpy.ndarray instances
pub const OPT_SERIALIZE_NUMPY: u32 = 1 << 9;

/// Serialize uuid.UUID instances
pub const OPT_SERIALIZE_UUID: u32 = 1 << 10;

/// Sort dict keys lexicographically
pub const OPT_SORT_KEYS: u32 = 1 << 11;

/// Enforce 53-bit integer limit (JavaScript safe integers)
pub const OPT_STRICT_INTEGER: u32 = 1 << 12;

/// Serialize UTC timezone as "Z" instead of "+00:00"
pub const OPT_UTC_Z: u32 = 1 << 13;

/// Parsed options for use in serialization
#[derive(Debug, Clone, Default)]
pub struct DumpOptions {
    pub append_newline: bool,
    pub indent: Option<usize>,
    pub naive_utc: bool,
    pub non_str_keys: bool,
    pub omit_microseconds: bool,
    pub passthrough_dataclass: bool,
    pub passthrough_datetime: bool,
    pub passthrough_subclass: bool,
    pub serialize_dataclass: bool,
    pub serialize_numpy: bool,
    pub serialize_uuid: bool,
    pub sort_keys: bool,
    pub strict_integer: bool,
    pub utc_z: bool,
}

impl DumpOptions {
    /// Parse option flags into structured options
    #[must_use]
    pub const fn from_flags(flags: u32) -> Self {
        Self {
            append_newline: flags & OPT_APPEND_NEWLINE != 0,
            indent: if flags & OPT_INDENT_2 != 0 {
                Some(2)
            } else {
                None
            },
            naive_utc: flags & OPT_NAIVE_UTC != 0,
            non_str_keys: flags & OPT_NON_STR_KEYS != 0,
            omit_microseconds: flags & OPT_OMIT_MICROSECONDS != 0,
            passthrough_dataclass: flags & OPT_PASSTHROUGH_DATACLASS != 0,
            passthrough_datetime: flags & OPT_PASSTHROUGH_DATETIME != 0,
            passthrough_subclass: flags & OPT_PASSTHROUGH_SUBCLASS != 0,
            serialize_dataclass: flags & OPT_SERIALIZE_DATACLASS != 0,
            serialize_numpy: flags & OPT_SERIALIZE_NUMPY != 0,
            serialize_uuid: flags & OPT_SERIALIZE_UUID != 0,
            sort_keys: flags & OPT_SORT_KEYS != 0,
            strict_integer: flags & OPT_STRICT_INTEGER != 0,
            utc_z: flags & OPT_UTC_Z != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values() {
        // Ensure flags match orjson exactly
        assert_eq!(OPT_APPEND_NEWLINE, 1);
        assert_eq!(OPT_INDENT_2, 2);
        assert_eq!(OPT_NAIVE_UTC, 4);
        assert_eq!(OPT_NON_STR_KEYS, 8);
        assert_eq!(OPT_OMIT_MICROSECONDS, 16);
        assert_eq!(OPT_PASSTHROUGH_DATACLASS, 32);
        assert_eq!(OPT_PASSTHROUGH_DATETIME, 64);
        assert_eq!(OPT_PASSTHROUGH_SUBCLASS, 128);
        assert_eq!(OPT_SERIALIZE_DATACLASS, 256);
        assert_eq!(OPT_SERIALIZE_NUMPY, 512);
        assert_eq!(OPT_SERIALIZE_UUID, 1024);
        assert_eq!(OPT_SORT_KEYS, 2048);
        assert_eq!(OPT_STRICT_INTEGER, 4096);
        assert_eq!(OPT_UTC_Z, 8192);
    }

    #[test]
    fn test_from_flags() {
        let opts = DumpOptions::from_flags(OPT_INDENT_2 | OPT_SORT_KEYS);
        assert_eq!(opts.indent, Some(2));
        assert!(opts.sort_keys);
        assert!(!opts.append_newline);
    }
}
