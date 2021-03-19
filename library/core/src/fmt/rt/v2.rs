//! This is an internal module used by the ifmt! runtime. These structures are
//! emitted to static arrays to precompile format strings ahead of time.
//!
//! These definitions are similar to their `ct` equivalents, but differ in that
//! these can be statically allocated and are slightly optimized for the runtime
#![allow(missing_debug_implementations)]

pub use super::v1::Alignment;

#[derive(Copy, Clone)]
pub struct Argument {
    pub position: usize,
    pub format: FormatSpec,
}

// todo inline?
#[derive(Copy, Clone)]
pub struct FormatSpec {
    pub fill: char,
    pub align: Alignment,
    pub flags: u32,
    pub precision: Option<usize>,
    pub width: Option<usize>,
}
