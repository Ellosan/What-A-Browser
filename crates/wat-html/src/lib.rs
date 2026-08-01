//! HTML parsing for What-A-Browser.
//!
//! ```
//! let doc = wat_html::parse("<h1>Hello</h1>");
//! let h1 = doc.query("h1").expect("heading");
//! assert_eq!(doc.text_content(h1), "Hello");
//! ```

pub mod entities;
pub mod tokenizer;
pub mod tree;

pub use tokenizer::{tokenize, Token, Tokenizer};
pub use tree::{parse, parse_with_base, TreeBuilder, VOID_ELEMENTS};
