// SPDX-License-Identifier: MIT OR Apache-2.0
//! Build script for fionn-py

fn main() {
    // Configure PyO3
    pyo3_build_config::add_extension_module_link_args();
}
