// `tree` lives in its own file rather than inside `types.rs` because
// `types.rs` is already at the 700-line hard ceiling — adding the
// ~80-line schema there would push it over. The pattern elsewhere in
// this module is to define section types in `types.rs`, but file-size
// budget pragmatism wins here.
pub mod tree;
pub mod types;

pub use tree::{TreeConfig, TreeStyle};

#[cfg(test)]
mod types_tests;
