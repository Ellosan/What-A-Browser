//! WAT's JavaScript engine.
//!
//! A lexer, a recursive-descent parser and a tree-walking interpreter for the
//! parts of ECMAScript a page actually uses. It is written from scratch for the
//! same reason the rest of the browser is: the whole path from source text to
//! side effect stays readable, and nothing in it is a black box.
//!
//! The engine knows nothing about the DOM. A host exposes its own objects by
//! implementing [`HostObject`] and installing them as globals, which is how
//! `wat-script` wires up `document` and `window`:
//!
//! ```
//! use wat_js::{Interp, Value};
//!
//! let mut interp = Interp::new();
//! interp.define_global("answer", Value::Number(42.0));
//! assert_eq!(
//!     interp.eval("`the answer is ${answer}`").unwrap().to_js_string(),
//!     "the answer is 42"
//! );
//! ```
//!
//! Two limits are enforced on every run, because a page script shares a thread
//! with the browser's own interface: a step budget and a maximum call depth.
//! Both surface as [`Control::Fatal`], which `try`/`catch` cannot swallow.
//!
//! ```
//! use wat_js::Interp;
//!
//! let mut interp = Interp::new();
//! let error = interp.eval("while (true) {}").unwrap_err();
//! assert!(error.contains("too long"));
//! ```
//!
//! # What is supported
//!
//! Expressions and operators, `var`/`let`/`const` with per-iteration loop
//! bindings, destructuring with defaults and rest elements, template literals,
//! spread, arrow functions, closures, `class` with inheritance, `super`,
//! `get`/`set` accessors, static and instance fields, `#private` fields,
//! `for`/`for-in`/`for-of`/`while`/`do`, `switch`, labelled `break` and
//! `continue`, `try`/`catch`/`finally`, `throw`, optional chaining, nullish
//! coalescing, logical assignment, exponentiation, and the built-ins in
//! [`builtins`].
//!
//! # What is not
//!
//! Regular expressions, promises and `async`/`await`, generators, `Symbol`,
//! `Proxy`, `Map`/`Set`/`WeakMap`, and modules. Strings are indexed by Unicode
//! scalar value rather than UTF-16 code unit, so an astral character counts as
//! one, not two.

pub mod ast;
pub mod builtins;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod value;

pub use ast::{Expr, Program, Stmt};
pub use interp::{
    host_value, native, ConsoleLevel, ConsoleMessage, Control, Interp, Limits, Timer,
};
pub use lexer::SyntaxError;
pub use parser::{parse, parse_expression};
pub use value::{inspect, HostObject, JsObject, Scope, Value};

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole pipeline, end to end, on a script that uses most of the
    /// language at once.
    #[test]
    fn a_realistic_script_runs() {
        let source = r#"
            class Shape {
                constructor(name) { this.name = name; }
                describe() { return `a ${this.name}`; }
            }
            class Square extends Shape {
                #sides = 4;
                constructor(size) {
                    super('square');
                    this.size = size;
                }
                get area() { return this.size * this.size; }
                describe() { return super.describe() + ` of ${this.area}`; }
            }

            const shapes = [new Square(2), new Square(3)];
            const areas = shapes.map(shape => shape.area).filter(area => area > 4);
            const { name, size } = shapes[1];
            const [first, ...rest] = areas;

            let total = 0;
            for (const area of areas) total += area;

            JSON.stringify({ name, size, first, rest, total, described: shapes[0].describe() })
        "#;
        let mut interp = Interp::new();
        let result = interp.eval(source).expect("the script should run");
        assert_eq!(
            result.to_js_string(),
            r#"{"name":"square","size":3,"first":9,"rest":[],"total":9,"described":"a square of 4"}"#
        );
    }

    #[test]
    fn a_host_object_is_reachable_from_script() {
        use std::cell::RefCell;

        struct Counter {
            count: RefCell<f64>,
        }

        impl HostObject for Counter {
            fn type_name(&self) -> String {
                "Counter".to_string()
            }

            fn get(&self, key: &str) -> Option<Value> {
                match key {
                    "count" => Some(Value::Number(*self.count.borrow())),
                    _ => None,
                }
            }

            fn set(&self, key: &str, value: &Value) -> bool {
                if key == "count" {
                    *self.count.borrow_mut() = value.to_number();
                    return true;
                }
                false
            }

            fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
                match method {
                    "add" => {
                        let amount = args.first().map(Value::to_number).unwrap_or(1.0);
                        *self.count.borrow_mut() += amount;
                        Ok(Value::Number(*self.count.borrow()))
                    }
                    other => Err(format!("Counter has no method `{other}`")),
                }
            }

            fn own_keys(&self) -> Vec<String> {
                vec!["count".to_string()]
            }

            fn identity(&self) -> usize {
                1
            }
        }

        let mut interp = Interp::new();
        interp.define_global(
            "counter",
            host_value(Counter {
                count: RefCell::new(0.0),
            }),
        );

        assert_eq!(
            interp
                .eval("counter.add(5); counter.count")
                .unwrap()
                .to_number(),
            5.0
        );
        assert_eq!(
            interp
                .eval("counter.count = 2; counter.count")
                .unwrap()
                .to_number(),
            2.0
        );
        assert_eq!(
            interp
                .eval("Object.keys(counter).toString()")
                .unwrap()
                .to_js_string(),
            "count"
        );

        let error = interp.eval("counter.missing()").unwrap_err();
        assert!(error.contains("no method"), "{error}");
    }

    #[test]
    fn a_syntax_error_reports_its_line() {
        let mut interp = Interp::new();
        let error = interp.eval("let a = 1;\nlet = ;").unwrap_err();
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn runaway_recursion_is_stopped_rather_than_overflowing_the_stack() {
        let mut interp = Interp::new();
        let error = interp.eval("function f() { return f() } f()").unwrap_err();
        assert!(error.contains("call depth"), "{error}");
    }

    #[test]
    fn a_fatal_limit_cannot_be_caught_by_the_script() {
        let mut interp = Interp::new();
        let error = interp
            .eval("try { while (true) {} } catch (e) { 'caught' }")
            .unwrap_err();
        assert!(error.contains("too long"), "{error}");
    }

    #[test]
    fn each_run_gets_a_fresh_budget() {
        let mut interp = Interp::new();
        interp
            .eval("let total = 0; for (let i = 0; i < 1000; i++) total += i")
            .unwrap();
        let used = interp.steps_used();
        assert!(used > 1000, "the loop should cost steps: {used}");
        interp.reset_budget();
        assert_eq!(interp.steps_used(), 0);
        // State survives the reset, which is what an event handler needs.
        assert_eq!(interp.eval("total").unwrap().to_number(), 499_500.0);
    }
}
