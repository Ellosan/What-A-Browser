//! Computed styles and the CSS cascade.
//!
//! [`StyleEngine`] holds the user-agent, user and author stylesheets for a
//! document and produces a [`StyleTree`] — one [`ComputedStyle`] per node, with
//! inheritance and specificity already resolved.

pub mod apply;
pub mod cascade;
pub mod types;

pub use apply::{apply_declaration, is_inherited};
pub use cascade::{StyleEngine, StyleTree, UA_STYLESHEET};
pub use types::*;
