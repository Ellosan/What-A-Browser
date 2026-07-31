//! CSS support for What-A-Browser: tokenizer, value types, selector engine,
//! media queries and the stylesheet parser.
//!
//! ```
//! use wat_css::{MediaContext, Origin, Stylesheet};
//!
//! let sheet = Stylesheet::parse("p.lead { color: #08f }", Origin::Author);
//! let rules = sheet.style_rules(&MediaContext::default());
//! assert_eq!(rules.len(), 1);
//! assert_eq!(rules[0].declarations[0].name, "color");
//! ```

pub mod color;
pub mod media;
pub mod parser;
pub mod selector;
pub mod tokenizer;
pub mod values;

pub use color::Color;
pub use media::{MediaContext, MediaQuery, MediaQueryList, MediaType};
pub use parser::{
    parse_declarations, parse_inline_style, Declaration, MediaRule, Origin, Rule, StyleRule,
    Stylesheet,
};
pub use selector::{
    parse_selector_list, AttrOp, AttrSelector, Combinator, Compound, MatchContext, Nth,
    PseudoClass, PseudoElement, Selector, Specificity,
};
pub use tokenizer::{tokenize, Token};
pub use values::{parse_value, parse_value_str, LengthContext, Unit, Value};
