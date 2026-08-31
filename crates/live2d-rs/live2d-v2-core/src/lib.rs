//! Safe Rust wrappers for the Live2D Cubism 2.x C API.
//!
//! This crate provides a safe, idiomatic Rust interface to V2 models,
//! mirroring the pattern used by `live2d-core` for V3.

pub use model::{clear_buffer, gl_init, Model};

pub mod model;
