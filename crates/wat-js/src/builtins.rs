//! The standard library.
//!
//! Everything here is written against the same public interpreter API a host
//! uses, so there is nothing a built-in can do that an embedder could not do
//! itself. Methods on primitives are not stored on prototype objects; they are
//! looked up on demand by [`string_member`] and friends, which keeps a string
//! or a number a plain Rust value with no wrapper allocation.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::interp::{native, ConsoleLevel, Control, Interp};
use crate::value::{format_number, inspect, JsObject, Value};

/// The error constructors, which also decide what `instanceof Error` accepts.
pub const ERROR_KINDS: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "EvalError",
    "URIError",
];

/// Installs the globals into a fresh interpreter.
pub fn install(interp: &mut Interp) {
    interp.define_global("undefined", Value::Undefined);
    interp.define_global("NaN", Value::Number(f64::NAN));
    interp.define_global("Infinity", Value::Number(f64::INFINITY));

    interp.define_global("console", console_object());
    interp.define_global("Math", math_object());
    interp.define_global("JSON", json_object());

    interp.define_global("Object", object_constructor());
    interp.define_global("Array", array_constructor());
    interp.define_global("String", string_constructor());
    interp.define_global("Number", number_constructor());
    interp.define_global("Boolean", boolean_constructor());
    interp.define_global("Date", date_constructor());

    for kind in ERROR_KINDS {
        interp.define_global(kind, error_constructor(kind));
    }

    interp.define_global(
        "parseInt",
        native("parseInt", |_, _, args| {
            Ok(Value::Number(parse_int(
                &arg(args, 0).to_js_string(),
                arg(args, 1),
            )))
        }),
    );
    interp.define_global(
        "parseFloat",
        native("parseFloat", |_, _, args| {
            Ok(Value::Number(parse_float(&arg(args, 0).to_js_string())))
        }),
    );
    interp.define_global(
        "isNaN",
        native("isNaN", |_, _, args| {
            Ok(Value::Bool(arg(args, 0).to_number().is_nan()))
        }),
    );
    interp.define_global(
        "isFinite",
        native("isFinite", |_, _, args| {
            Ok(Value::Bool(arg(args, 0).to_number().is_finite()))
        }),
    );

    interp.define_global(
        "encodeURIComponent",
        native("encodeURIComponent", |_, _, args| {
            Ok(Value::string(encode_uri(
                &arg(args, 0).to_js_string(),
                COMPONENT_SAFE,
            )))
        }),
    );
    interp.define_global(
        "encodeURI",
        native("encodeURI", |_, _, args| {
            Ok(Value::string(encode_uri(
                &arg(args, 0).to_js_string(),
                URI_SAFE,
            )))
        }),
    );
    for name in ["decodeURIComponent", "decodeURI"] {
        let decoder = native("decodeURIComponent", |interp, _, args| {
            match decode_uri(&arg(args, 0).to_js_string()) {
                Some(text) => Ok(Value::string(text)),
                None => Err(interp.throw("URIError", "URI malformed")),
            }
        });
        interp.define_global(name, decoder);
    }

    interp.define_global(
        "setTimeout",
        native("setTimeout", |interp, _, args| {
            let callback = arg(args, 0);
            if !callback.is_callable() {
                return Err(interp.type_error("setTimeout expects a function"));
            }
            let delay = arg(args, 1).to_number();
            let extra: Vec<Value> = args.iter().skip(2).cloned().collect();
            let id = interp.queue_timer(callback, sanitise_delay(delay), extra, false);
            Ok(Value::Number(id as f64))
        }),
    );
    interp.define_global(
        "setInterval",
        native("setInterval", |interp, _, args| {
            let callback = arg(args, 0);
            if !callback.is_callable() {
                return Err(interp.type_error("setInterval expects a function"));
            }
            let delay = arg(args, 1).to_number();
            let extra: Vec<Value> = args.iter().skip(2).cloned().collect();
            let id = interp.queue_timer(callback, sanitise_delay(delay), extra, true);
            Ok(Value::Number(id as f64))
        }),
    );
    for name in ["clearTimeout", "clearInterval"] {
        let clear = native("clearTimeout", |interp, _, args| {
            let id = arg(args, 0).to_number();
            if id.is_finite() && id >= 0.0 {
                interp.cancel_timer(id as u32);
            }
            Ok(Value::Undefined)
        });
        interp.define_global(name, clear);
    }
}

/// A delay a script asked for, clamped to something a host can honour.
fn sanitise_delay(delay: f64) -> f64 {
    if delay.is_finite() && delay > 0.0 {
        delay.min(60_000.0)
    } else {
        0.0
    }
}

/// Reads an argument, treating a missing one as `undefined`.
fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or(Value::Undefined)
}

/// Hangs properties on a native function, for the namespace-style built-ins.
fn with_statics(value: Value, entries: Vec<(&str, Value)>) -> Value {
    if let Value::Native(function) = &value {
        for (key, entry) in entries {
            function.properties.set(key, entry);
        }
    }
    value
}

/// Builds a plain object from a list of entries.
fn object_of(class: &str, entries: Vec<(&str, Value)>) -> Value {
    let object = JsObject::with_class(class);
    for (key, value) in entries {
        object.set(key, value);
    }
    Value::object(object)
}

// ---- console --------------------------------------------------------------

fn console_object() -> Value {
    object_of(
        "console",
        vec![
            ("log", console_method("log", ConsoleLevel::Log)),
            ("debug", console_method("debug", ConsoleLevel::Log)),
            ("dir", console_method("dir", ConsoleLevel::Log)),
            ("info", console_method("info", ConsoleLevel::Info)),
            ("warn", console_method("warn", ConsoleLevel::Warn)),
            ("error", console_method("error", ConsoleLevel::Error)),
            ("trace", console_method("trace", ConsoleLevel::Warn)),
        ],
    )
}

fn console_method(name: &'static str, level: ConsoleLevel) -> Value {
    native(name, move |interp, _this, args| {
        let text = args.iter().map(inspect).collect::<Vec<_>>().join(" ");
        interp.log(level, text);
        Ok(Value::Undefined)
    })
}

// ---- Math -----------------------------------------------------------------

fn math_object() -> Value {
    object_of(
        "Math",
        vec![
            ("PI", Value::Number(std::f64::consts::PI)),
            ("E", Value::Number(std::f64::consts::E)),
            ("LN2", Value::Number(std::f64::consts::LN_2)),
            ("LN10", Value::Number(std::f64::consts::LN_10)),
            ("LOG2E", Value::Number(std::f64::consts::LOG2_E)),
            ("LOG10E", Value::Number(std::f64::consts::LOG10_E)),
            ("SQRT2", Value::Number(std::f64::consts::SQRT_2)),
            ("SQRT1_2", Value::Number(std::f64::consts::FRAC_1_SQRT_2)),
            ("abs", math_unary("abs", |x| x.abs())),
            ("floor", math_unary("floor", |x| x.floor())),
            ("ceil", math_unary("ceil", |x| x.ceil())),
            ("trunc", math_unary("trunc", |x| x.trunc())),
            ("sqrt", math_unary("sqrt", |x| x.sqrt())),
            ("cbrt", math_unary("cbrt", |x| x.cbrt())),
            ("exp", math_unary("exp", |x| x.exp())),
            ("log", math_unary("log", |x| x.ln())),
            ("log2", math_unary("log2", |x| x.log2())),
            ("log10", math_unary("log10", |x| x.log10())),
            ("sin", math_unary("sin", |x| x.sin())),
            ("cos", math_unary("cos", |x| x.cos())),
            ("tan", math_unary("tan", |x| x.tan())),
            ("asin", math_unary("asin", |x| x.asin())),
            ("acos", math_unary("acos", |x| x.acos())),
            ("atan", math_unary("atan", |x| x.atan())),
            ("sinh", math_unary("sinh", |x| x.sinh())),
            ("cosh", math_unary("cosh", |x| x.cosh())),
            ("tanh", math_unary("tanh", |x| x.tanh())),
            // JavaScript rounds halves towards positive infinity, which is not
            // what Rust's `round` does for negative numbers.
            (
                "round",
                math_unary(
                    "round",
                    |x| {
                        if x.is_finite() {
                            (x + 0.5).floor()
                        } else {
                            x
                        }
                    },
                ),
            ),
            (
                "sign",
                math_unary("sign", |x| {
                    if x.is_nan() || x == 0.0 {
                        x
                    } else if x > 0.0 {
                        1.0
                    } else {
                        -1.0
                    }
                }),
            ),
            (
                "pow",
                native("pow", |_, _, args| {
                    Ok(Value::Number(
                        arg(args, 0).to_number().powf(arg(args, 1).to_number()),
                    ))
                }),
            ),
            (
                "atan2",
                native("atan2", |_, _, args| {
                    Ok(Value::Number(
                        arg(args, 0).to_number().atan2(arg(args, 1).to_number()),
                    ))
                }),
            ),
            (
                "hypot",
                native("hypot", |_, _, args| {
                    let sum: f64 = args.iter().map(|value| value.to_number().powi(2)).sum();
                    Ok(Value::Number(sum.sqrt()))
                }),
            ),
            (
                "min",
                native("min", |_, _, args| Ok(Value::Number(extremum(args, true)))),
            ),
            (
                "max",
                native("max", |_, _, args| Ok(Value::Number(extremum(args, false)))),
            ),
            (
                "random",
                native("random", |_, _, _| Ok(Value::Number(random()))),
            ),
        ],
    )
}

fn math_unary(name: &'static str, function: fn(f64) -> f64) -> Value {
    native(name, move |_, _, args| {
        Ok(Value::Number(function(arg(args, 0).to_number())))
    })
}

fn extremum(args: &[Value], minimum: bool) -> f64 {
    let mut best = if minimum {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    };
    for value in args {
        let number = value.to_number();
        if number.is_nan() {
            return f64::NAN;
        }
        if (minimum && number < best) || (!minimum && number > best) {
            best = number;
        }
    }
    best
}

thread_local! {
    /// The `Math.random` state. Seeded from the clock, so two runs differ, and
    /// kept per-thread so no locking is needed on the render thread.
    static RANDOM_STATE: RefCell<u64> = RefCell::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0x2545_f491_4f6c_dd1d)
            | 1,
    );
}

/// An xorshift64\* generator, which is plenty for page scripts and avoids a
/// dependency.
fn random() -> f64 {
    RANDOM_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let mut x = *state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *state = x;
        // The top 53 bits give a uniform double in [0, 1).
        ((x.wrapping_mul(0x2545_f491_4f6c_dd1d)) >> 11) as f64 / (1u64 << 53) as f64
    })
}

// ---- JSON -----------------------------------------------------------------

fn json_object() -> Value {
    object_of(
        "JSON",
        vec![
            (
                "stringify",
                native("stringify", |interp, _, args| {
                    let value = arg(args, 0);
                    let indent = match arg(args, 2) {
                        Value::Number(count) if count >= 1.0 => {
                            " ".repeat(count.min(10.0) as usize)
                        }
                        Value::Str(text) => text.chars().take(10).collect(),
                        _ => String::new(),
                    };
                    Ok(match stringify(interp, &value, &indent, 0) {
                        Some(text) => Value::string(text),
                        None => Value::Undefined,
                    })
                }),
            ),
            (
                "parse",
                native("parse", |interp, _, args| {
                    let text = arg(args, 0).to_js_string();
                    let mut parser = JsonParser {
                        chars: text.chars().collect(),
                        index: 0,
                    };
                    parser.skip_whitespace();
                    let value = parser
                        .value()
                        .ok_or_else(|| interp.throw("SyntaxError", "invalid JSON"))?;
                    parser.skip_whitespace();
                    if parser.index != parser.chars.len() {
                        return Err(interp.throw("SyntaxError", "unexpected trailing JSON"));
                    }
                    Ok(value)
                }),
            ),
        ],
    )
}

/// Serialises a value, returning `None` for the values JSON omits.
fn stringify(interp: &mut Interp, value: &Value, indent: &str, depth: usize) -> Option<String> {
    // A cap instead of cycle detection: deep or cyclic structures stop rather
    // than recursing until the stack gives out.
    if depth > 64 {
        return Some("null".to_string());
    }
    let (open, close, separator, colon) = if indent.is_empty() {
        (
            String::new(),
            String::new(),
            ",".to_string(),
            ":".to_string(),
        )
    } else {
        let inner = indent.repeat(depth + 1);
        let outer = indent.repeat(depth);
        (
            format!("\n{inner}"),
            format!("\n{outer}"),
            format!(",\n{inner}"),
            ": ".to_string(),
        )
    };

    match value {
        Value::Undefined | Value::Function(_) | Value::Native(_) => None,
        Value::Null => Some("null".to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(if number.is_finite() {
            format_number(*number)
        } else {
            "null".to_string()
        }),
        Value::Str(text) => Some(quote_json(text)),
        Value::Array(items) => {
            let items = items.borrow().clone();
            if items.is_empty() {
                return Some("[]".to_string());
            }
            let rendered: Vec<String> = items
                .iter()
                .map(|item| {
                    stringify(interp, item, indent, depth + 1).unwrap_or_else(|| "null".to_string())
                })
                .collect();
            Some(format!("[{open}{}{close}]", rendered.join(&separator)))
        }
        Value::Object(_) | Value::Host(_) => {
            let keys = interp.enumerate_keys(value);
            let mut rendered = Vec::new();
            for key in keys {
                let Ok(property) = interp.get_member(value, &key) else {
                    continue;
                };
                if let Some(text) = stringify(interp, &property, indent, depth + 1) {
                    rendered.push(format!("{}{colon}{text}", quote_json(&key)));
                }
            }
            if rendered.is_empty() {
                return Some("{}".to_string());
            }
            Some(format!("{{{open}{}{close}}}", rendered.join(&separator)))
        }
    }
}

fn quote_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

struct JsonParser {
    chars: Vec<char>,
    index: usize,
}

impl JsonParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn literal(&mut self, text: &str) -> bool {
        if self.chars[self.index..].starts_with(&text.chars().collect::<Vec<_>>()[..]) {
            self.index += text.chars().count();
            return true;
        }
        false
    }

    fn value(&mut self) -> Option<Value> {
        self.skip_whitespace();
        match self.peek()? {
            'n' => self.literal("null").then_some(Value::Null),
            't' => self.literal("true").then_some(Value::Bool(true)),
            'f' => self.literal("false").then_some(Value::Bool(false)),
            '"' => self.string().map(Value::string),
            '[' => self.array(),
            '{' => self.object(),
            _ => self.number(),
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != '"' {
            return None;
        }
        self.index += 1;
        let mut out = String::new();
        loop {
            let ch = self.peek()?;
            self.index += 1;
            match ch {
                '"' => return Some(out),
                '\\' => {
                    let escape = self.peek()?;
                    self.index += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let digit = self.peek()?.to_digit(16)?;
                                self.index += 1;
                                code = code * 16 + digit;
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                }
                ch => out.push(ch),
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.index;
        if self.peek() == Some('-') {
            self.index += 1;
        }
        while matches!(self.peek(), Some('0'..='9' | '.' | 'e' | 'E' | '+' | '-')) {
            self.index += 1;
        }
        if self.index == start {
            return None;
        }
        let text: String = self.chars[start..self.index].iter().collect();
        text.parse().ok().map(Value::Number)
    }

    fn array(&mut self) -> Option<Value> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.index += 1;
            return Some(Value::array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek()? {
                ',' => self.index += 1,
                ']' => {
                    self.index += 1;
                    return Some(Value::array(items));
                }
                _ => return None,
            }
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.index += 1;
        let object = JsObject::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.index += 1;
            return Some(Value::object(object));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek()? != ':' {
                return None;
            }
            self.index += 1;
            object.set(key, self.value()?);
            self.skip_whitespace();
            match self.peek()? {
                ',' => self.index += 1,
                '}' => {
                    self.index += 1;
                    return Some(Value::object(object));
                }
                _ => return None,
            }
        }
    }
}

// ---- constructors ---------------------------------------------------------

fn error_constructor(kind: &'static str) -> Value {
    native(kind, move |interp, _this, args| {
        let message = match args.first() {
            Some(value) if !value.is_nullish() => value.to_js_string(),
            _ => String::new(),
        };
        Ok(interp.make_error(kind, message))
    })
}

fn object_constructor() -> Value {
    let constructor = native("Object", |_, _, args| {
        Ok(match arg(args, 0) {
            Value::Undefined | Value::Null => Value::object(JsObject::new()),
            other => other,
        })
    });
    with_statics(
        constructor,
        vec![
            (
                "keys",
                native("keys", |interp, _, args| {
                    let keys = interp
                        .enumerate_keys(&arg(args, 0))
                        .into_iter()
                        .map(Value::string)
                        .collect();
                    Ok(Value::array(keys))
                }),
            ),
            (
                "values",
                native("values", |interp, _, args| {
                    let target = arg(args, 0);
                    let mut values = Vec::new();
                    for key in interp.enumerate_keys(&target) {
                        values.push(interp.get_member(&target, &key)?);
                    }
                    Ok(Value::array(values))
                }),
            ),
            (
                "entries",
                native("entries", |interp, _, args| {
                    let target = arg(args, 0);
                    let mut entries = Vec::new();
                    for key in interp.enumerate_keys(&target) {
                        let value = interp.get_member(&target, &key)?;
                        entries.push(Value::array(vec![Value::string(key), value]));
                    }
                    Ok(Value::array(entries))
                }),
            ),
            (
                "assign",
                native("assign", |interp, _, args| {
                    let target = arg(args, 0);
                    for source in args.iter().skip(1) {
                        if source.is_nullish() {
                            continue;
                        }
                        for key in interp.enumerate_keys(source) {
                            let value = interp.get_member(source, &key)?;
                            interp.set_member(&target, &key, value)?;
                        }
                    }
                    Ok(target)
                }),
            ),
            (
                "fromEntries",
                native("fromEntries", |interp, _, args| {
                    let object = JsObject::new();
                    for entry in interp.iterate(&arg(args, 0))? {
                        let key = interp.get_member(&entry, "0")?.to_property_key();
                        let value = interp.get_member(&entry, "1")?;
                        object.set(key, value);
                    }
                    Ok(Value::object(object))
                }),
            ),
            (
                "create",
                native("create", |interp, _, args| {
                    let object = JsObject::new();
                    match arg(args, 0) {
                        Value::Object(prototype) => object.set_prototype(Some(prototype)),
                        Value::Null => {}
                        other => {
                            return Err(interp.type_error(format!(
                                "Object.create expects an object or null, not {}",
                                other.type_of()
                            )))
                        }
                    }
                    let object = Value::object(object);
                    if let Value::Object(descriptors) = arg(args, 1) {
                        for (key, descriptor) in descriptors.entries() {
                            if let Some(value) = descriptor_value(interp, &descriptor)? {
                                interp.set_member(&object, &key, value)?;
                            }
                        }
                    }
                    Ok(object)
                }),
            ),
            (
                "defineProperty",
                native("defineProperty", |interp, _, args| {
                    let target = arg(args, 0);
                    let key = arg(args, 1).to_property_key();
                    let descriptor = arg(args, 2);
                    if let Some(value) = descriptor_value(interp, &descriptor)? {
                        interp.set_member(&target, &key, value)?;
                    }
                    Ok(target)
                }),
            ),
            (
                "getPrototypeOf",
                native("getPrototypeOf", |_, _, args| {
                    Ok(match arg(args, 0) {
                        Value::Object(object) => match object.prototype() {
                            Some(prototype) => Value::Object(prototype),
                            None => Value::Null,
                        },
                        Value::Function(closure) => Value::Object(closure.prototype.clone()),
                        _ => Value::Null,
                    })
                }),
            ),
            (
                "setPrototypeOf",
                native("setPrototypeOf", |_, _, args| {
                    let target = arg(args, 0);
                    if let Value::Object(object) = &target {
                        match arg(args, 1) {
                            Value::Object(prototype) => object.set_prototype(Some(prototype)),
                            _ => object.set_prototype(None),
                        }
                    }
                    Ok(target)
                }),
            ),
            (
                "hasOwn",
                native("hasOwn", |_, _, args| {
                    Ok(Value::Bool(has_own(
                        &arg(args, 0),
                        &arg(args, 1).to_property_key(),
                    )))
                }),
            ),
            // Freezing is not enforced; scripts that only use it defensively
            // still work, and nothing here relies on immutability.
            ("freeze", native("freeze", |_, _, args| Ok(arg(args, 0)))),
            (
                "isFrozen",
                native("isFrozen", |_, _, _| Ok(Value::Bool(false))),
            ),
        ],
    )
}

/// Reads the `value` out of a property descriptor.
fn descriptor_value(interp: &mut Interp, descriptor: &Value) -> Result<Option<Value>, Control> {
    if let Value::Object(object) = descriptor {
        if let Some(value) = object.own("value") {
            return Ok(Some(value));
        }
        // Accessors are not supported, so a getter is called once and its
        // result stored. That covers computed constants, which is the common
        // use, and is documented behaviour rather than a silent failure.
        if let Some(getter) = object.own("get") {
            if getter.is_callable() {
                return Ok(Some(interp.call(&getter, descriptor.clone(), &[])?));
            }
        }
    }
    Ok(None)
}

fn has_own(target: &Value, key: &str) -> bool {
    match target {
        Value::Object(object) => object.has_own(key),
        Value::Array(items) => key
            .parse::<usize>()
            .map(|index| index < items.borrow().len())
            .unwrap_or(key == "length"),
        Value::Str(text) => key
            .parse::<usize>()
            .map(|index| index < text.chars().count())
            .unwrap_or(key == "length"),
        Value::Function(closure) => closure.properties.has_own(key),
        Value::Native(function) => function.properties.has_own(key),
        Value::Host(host) => host.own_keys().iter().any(|name| name == key),
        _ => false,
    }
}

fn array_constructor() -> Value {
    let constructor = native("Array", |_, _, args| {
        // `Array(5)` is a length; `Array(1, 2)` is a list of elements.
        if args.len() == 1 {
            if let Value::Number(length) = args[0] {
                if length >= 0.0 && length.fract() == 0.0 && length < 100_000_000.0 {
                    return Ok(Value::array(vec![Value::Undefined; length as usize]));
                }
            }
        }
        Ok(Value::array(args.to_vec()))
    });
    with_statics(
        constructor,
        vec![
            (
                "isArray",
                native("isArray", |_, _, args| {
                    Ok(Value::Bool(matches!(arg(args, 0), Value::Array(_))))
                }),
            ),
            (
                "of",
                native("of", |_, _, args| Ok(Value::array(args.to_vec()))),
            ),
            (
                "from",
                native("from", |interp, _, args| {
                    let source = arg(args, 0);
                    let items = interp.iterate(&source)?;
                    let mapper = arg(args, 1);
                    if !mapper.is_callable() {
                        return Ok(Value::array(items));
                    }
                    let mut mapped = Vec::with_capacity(items.len());
                    for (index, item) in items.into_iter().enumerate() {
                        mapped.push(interp.call(
                            &mapper,
                            Value::Undefined,
                            &[item, Value::Number(index as f64)],
                        )?);
                    }
                    Ok(Value::array(mapped))
                }),
            ),
        ],
    )
}

fn string_constructor() -> Value {
    let constructor = native("String", |_, _, args| {
        Ok(Value::string(match args.first() {
            Some(value) => value.to_js_string(),
            None => String::new(),
        }))
    });
    with_statics(
        constructor,
        vec![
            (
                "fromCharCode",
                native("fromCharCode", |_, _, args| {
                    let text: String = args
                        .iter()
                        .filter_map(|value| char::from_u32(value.to_uint32()))
                        .collect();
                    Ok(Value::string(text))
                }),
            ),
            (
                "fromCodePoint",
                native("fromCodePoint", |_, _, args| {
                    let text: String = args
                        .iter()
                        .filter_map(|value| char::from_u32(value.to_uint32()))
                        .collect();
                    Ok(Value::string(text))
                }),
            ),
        ],
    )
}

fn number_constructor() -> Value {
    let constructor = native("Number", |_, _, args| {
        Ok(Value::Number(match args.first() {
            Some(value) => value.to_number(),
            None => 0.0,
        }))
    });
    with_statics(
        constructor,
        vec![
            (
                "isInteger",
                native("isInteger", |_, _, args| {
                    Ok(Value::Bool(match arg(args, 0) {
                        Value::Number(number) => number.is_finite() && number.fract() == 0.0,
                        _ => false,
                    }))
                }),
            ),
            (
                "isSafeInteger",
                native("isSafeInteger", |_, _, args| {
                    Ok(Value::Bool(match arg(args, 0) {
                        Value::Number(number) => {
                            number.is_finite()
                                && number.fract() == 0.0
                                && number.abs() <= 9_007_199_254_740_991.0
                        }
                        _ => false,
                    }))
                }),
            ),
            (
                "isFinite",
                native("isFinite", |_, _, args| {
                    Ok(Value::Bool(
                        matches!(arg(args, 0), Value::Number(number) if number.is_finite()),
                    ))
                }),
            ),
            (
                "isNaN",
                native("isNaN", |_, _, args| {
                    Ok(Value::Bool(
                        matches!(arg(args, 0), Value::Number(number) if number.is_nan()),
                    ))
                }),
            ),
            (
                "parseFloat",
                native("parseFloat", |_, _, args| {
                    Ok(Value::Number(parse_float(&arg(args, 0).to_js_string())))
                }),
            ),
            (
                "parseInt",
                native("parseInt", |_, _, args| {
                    Ok(Value::Number(parse_int(
                        &arg(args, 0).to_js_string(),
                        arg(args, 1),
                    )))
                }),
            ),
            ("EPSILON", Value::Number(f64::EPSILON)),
            ("MAX_SAFE_INTEGER", Value::Number(9_007_199_254_740_991.0)),
            ("MIN_SAFE_INTEGER", Value::Number(-9_007_199_254_740_991.0)),
            ("MAX_VALUE", Value::Number(f64::MAX)),
            ("MIN_VALUE", Value::Number(5e-324)),
            ("POSITIVE_INFINITY", Value::Number(f64::INFINITY)),
            ("NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY)),
            ("NaN", Value::Number(f64::NAN)),
        ],
    )
}

fn boolean_constructor() -> Value {
    native("Boolean", |_, _, args| {
        Ok(Value::Bool(arg(args, 0).truthy()))
    })
}

// ---- Date -----------------------------------------------------------------

fn now_milliseconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or(0.0)
}

fn date_constructor() -> Value {
    let constructor = native("Date", |_, _, args| {
        let milliseconds = match args.first() {
            Some(value) => value.to_number(),
            None => now_milliseconds(),
        };
        Ok(date_object(milliseconds))
    });
    with_statics(
        constructor,
        vec![(
            "now",
            native("now", |_, _, _| Ok(Value::Number(now_milliseconds()))),
        )],
    )
}

/// A date is a frozen instant with its accessors bound to it, so there is no
/// hidden state to keep in sync.
///
/// There is no time-zone database here, so the local-time accessors report UTC.
fn date_object(milliseconds: f64) -> Value {
    let parts = DateParts::from_milliseconds(milliseconds);
    let object = JsObject::with_class("Date");
    let accessors: Vec<(&str, f64)> = vec![
        ("getTime", milliseconds),
        ("valueOf", milliseconds),
        ("getFullYear", parts.year as f64),
        ("getMonth", parts.month as f64 - 1.0),
        ("getDate", parts.day as f64),
        ("getDay", parts.weekday as f64),
        ("getHours", parts.hour as f64),
        ("getMinutes", parts.minute as f64),
        ("getSeconds", parts.second as f64),
        ("getMilliseconds", parts.millisecond as f64),
        ("getTimezoneOffset", 0.0),
        ("getUTCFullYear", parts.year as f64),
        ("getUTCMonth", parts.month as f64 - 1.0),
        ("getUTCDate", parts.day as f64),
        ("getUTCHours", parts.hour as f64),
        ("getUTCMinutes", parts.minute as f64),
        ("getUTCSeconds", parts.second as f64),
    ];
    for (name, value) in accessors {
        object.set(
            name,
            native("dateAccessor", move |_, _, _| Ok(Value::Number(value))),
        );
    }
    let iso = parts.to_iso();
    for name in ["toISOString", "toJSON", "toString"] {
        let iso = iso.clone();
        object.set(
            name,
            native("toISOString", move |_, _, _| Ok(Value::string(iso.clone()))),
        );
    }
    Value::object(object)
}

struct DateParts {
    year: i64,
    month: u32,
    day: u32,
    weekday: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

impl DateParts {
    fn from_milliseconds(milliseconds: f64) -> DateParts {
        let total = if milliseconds.is_finite() {
            milliseconds as i64
        } else {
            0
        };
        // Rust's `%` truncates towards zero, so pre-epoch instants need the
        // remainder nudged back into range.
        let mut day_number = total.div_euclid(86_400_000);
        let mut in_day = total.rem_euclid(86_400_000);
        if in_day < 0 {
            in_day += 86_400_000;
            day_number -= 1;
        }
        let (year, month, day) = civil_from_days(day_number);
        DateParts {
            year,
            month,
            day,
            // 1970-01-01 was a Thursday.
            weekday: (day_number + 4).rem_euclid(7) as u32,
            hour: (in_day / 3_600_000) as u32,
            minute: (in_day / 60_000 % 60) as u32,
            second: (in_day / 1_000 % 60) as u32,
            millisecond: (in_day % 1_000) as u32,
        }
    }

    fn to_iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millisecond
        )
    }
}

/// Days since 1970-01-01 to a calendar date, using Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---- instanceof for the native constructors -------------------------------

/// Whether `value instanceof <native constructor>` holds.
pub fn native_instance_of(constructor: &str, value: &Value) -> bool {
    match constructor {
        "Array" => matches!(value, Value::Array(_)),
        "Function" => value.is_callable(),
        "String" => matches!(value, Value::Str(_)),
        "Number" => matches!(value, Value::Number(_)),
        "Boolean" => matches!(value, Value::Bool(_)),
        "Date" => matches!(value, Value::Object(object) if *object.class_name.borrow() == "Date"),
        "Object" => !matches!(
            value,
            Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) | Value::Str(_)
        ),
        kind if ERROR_KINDS.contains(&kind) => match value {
            Value::Object(object) => {
                let class = object.class_name.borrow().clone();
                if kind == "Error" {
                    ERROR_KINDS.contains(&class.as_str())
                } else {
                    class == kind
                }
            }
            _ => false,
        },
        _ => false,
    }
}

// ---- string methods -------------------------------------------------------

fn characters(text: &str) -> Vec<char> {
    text.chars().collect()
}

/// Resolves a possibly-negative, possibly-missing index against a length, the
/// way `slice` does.
fn relative_index(value: &Value, length: usize, default: usize) -> usize {
    match value {
        Value::Undefined => default,
        other => {
            let index = other.to_number();
            if index.is_nan() {
                return 0;
            }
            if index < 0.0 {
                let from_end = length as f64 + index;
                from_end.max(0.0) as usize
            } else {
                (index as usize).min(length)
            }
        }
    }
}

/// The methods available on a string.
pub fn string_member(key: &str) -> Option<Value> {
    let method = match key {
        "charAt" => native("charAt", |_, this, args| {
            let chars = characters(&this.to_js_string());
            let index = arg(args, 0).to_number();
            Ok(Value::string(
                if index >= 0.0 && (index as usize) < chars.len() {
                    chars[index as usize].to_string()
                } else {
                    String::new()
                },
            ))
        }),
        "charCodeAt" | "codePointAt" => native("charCodeAt", |_, this, args| {
            let chars = characters(&this.to_js_string());
            let index = arg(args, 0).to_number().max(0.0) as usize;
            Ok(match chars.get(index) {
                Some(ch) => Value::Number(*ch as u32 as f64),
                None => Value::Number(f64::NAN),
            })
        }),
        "at" => native("at", |_, this, args| {
            let chars = characters(&this.to_js_string());
            let index = arg(args, 0).to_number();
            let resolved = if index < 0.0 {
                chars.len() as f64 + index
            } else {
                index
            };
            if resolved < 0.0 || resolved as usize >= chars.len() {
                return Ok(Value::Undefined);
            }
            Ok(Value::string(chars[resolved as usize].to_string()))
        }),
        "concat" => native("concat", |_, this, args| {
            let mut text = this.to_js_string();
            for value in args {
                text.push_str(&value.to_js_string());
            }
            Ok(Value::string(text))
        }),
        "includes" => native("includes", |_, this, args| {
            Ok(Value::Bool(
                this.to_js_string().contains(&arg(args, 0).to_js_string()),
            ))
        }),
        "indexOf" => native("indexOf", |_, this, args| {
            let text = this.to_js_string();
            let needle = arg(args, 0).to_js_string();
            let from = relative_index(&arg(args, 1), text.chars().count(), 0);
            let byte_start = char_to_byte(&text, from);
            Ok(Value::Number(match text[byte_start..].find(&needle) {
                Some(offset) => text[..byte_start + offset].chars().count() as f64,
                None => -1.0,
            }))
        }),
        "lastIndexOf" => native("lastIndexOf", |_, this, args| {
            let text = this.to_js_string();
            let needle = arg(args, 0).to_js_string();
            Ok(Value::Number(match text.rfind(&needle) {
                Some(offset) => text[..offset].chars().count() as f64,
                None => -1.0,
            }))
        }),
        "startsWith" => native("startsWith", |_, this, args| {
            let text = this.to_js_string();
            let offset = char_to_byte(
                &text,
                relative_index(&arg(args, 1), text.chars().count(), 0),
            );
            Ok(Value::Bool(
                text[offset..].starts_with(&arg(args, 0).to_js_string()),
            ))
        }),
        "endsWith" => native("endsWith", |_, this, args| {
            let text = this.to_js_string();
            let length = text.chars().count();
            let end = char_to_byte(&text, relative_index(&arg(args, 1), length, length));
            Ok(Value::Bool(
                text[..end].ends_with(&arg(args, 0).to_js_string()),
            ))
        }),
        "slice" => native("slice", |_, this, args| {
            let text = this.to_js_string();
            let length = text.chars().count();
            let start = relative_index(&arg(args, 0), length, 0);
            let end = relative_index(&arg(args, 1), length, length);
            Ok(Value::string(if start >= end {
                String::new()
            } else {
                text.chars()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>()
            }))
        }),
        "substring" => native("substring", |_, this, args| {
            let text = this.to_js_string();
            let length = text.chars().count();
            // Unlike `slice`, negative arguments clamp to zero and a reversed
            // range is swapped.
            let clamp = |value: Value| -> usize {
                let number = value.to_number();
                if number.is_nan() || number < 0.0 {
                    0
                } else {
                    (number as usize).min(length)
                }
            };
            let mut start = clamp(arg(args, 0));
            let mut end = match arg(args, 1) {
                Value::Undefined => length,
                other => clamp(other),
            };
            if start > end {
                std::mem::swap(&mut start, &mut end);
            }
            Ok(Value::string(
                text.chars()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>(),
            ))
        }),
        "substr" => native("substr", |_, this, args| {
            let text = this.to_js_string();
            let length = text.chars().count();
            let start = relative_index(&arg(args, 0), length, 0);
            let count = match arg(args, 1) {
                Value::Undefined => length - start,
                other => other.to_number().max(0.0) as usize,
            };
            Ok(Value::string(
                text.chars().skip(start).take(count).collect::<String>(),
            ))
        }),
        "split" => native("split", |_, this, args| {
            let text = this.to_js_string();
            let limit = match arg(args, 1) {
                Value::Undefined => usize::MAX,
                other => other.to_number().max(0.0) as usize,
            };
            let parts: Vec<Value> = match arg(args, 0) {
                Value::Undefined => vec![Value::string(text)],
                separator => {
                    let separator = separator.to_js_string();
                    if separator.is_empty() {
                        text.chars()
                            .map(|ch| Value::string(ch.to_string()))
                            .collect()
                    } else {
                        text.split(separator.as_str()).map(Value::string).collect()
                    }
                }
            };
            Ok(Value::array(parts.into_iter().take(limit).collect()))
        }),
        "repeat" => native("repeat", |interp, this, args| {
            let count = arg(args, 0).to_number();
            if count < 0.0 || !count.is_finite() {
                return Err(interp.range_error("repeat count must be finite and non-negative"));
            }
            let text = this.to_js_string();
            // A script must not be able to allocate an enormous string.
            if text.len().saturating_mul(count as usize) > 1 << 24 {
                return Err(interp.range_error("repeated string is too long"));
            }
            Ok(Value::string(text.repeat(count as usize)))
        }),
        "padStart" => pad_method(true),
        "padEnd" => pad_method(false),
        "replace" => replace_method(false),
        "replaceAll" => replace_method(true),
        "toLowerCase" | "toLocaleLowerCase" => native("toLowerCase", |_, this, _| {
            Ok(Value::string(this.to_js_string().to_lowercase()))
        }),
        "toUpperCase" | "toLocaleUpperCase" => native("toUpperCase", |_, this, _| {
            Ok(Value::string(this.to_js_string().to_uppercase()))
        }),
        "trim" => native("trim", |_, this, _| {
            Ok(Value::string(this.to_js_string().trim().to_string()))
        }),
        "trimStart" => native("trimStart", |_, this, _| {
            Ok(Value::string(this.to_js_string().trim_start().to_string()))
        }),
        "trimEnd" => native("trimEnd", |_, this, _| {
            Ok(Value::string(this.to_js_string().trim_end().to_string()))
        }),
        "localeCompare" => native("localeCompare", |_, this, args| {
            let left = this.to_js_string();
            let right = arg(args, 0).to_js_string();
            Ok(Value::Number(match left.cmp(&right) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }))
        }),
        // Every string here is already a Rust `String`, which is valid UTF-8,
        // so normalisation has nothing to do.
        "normalize" => native("normalize", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        }),
        "toString" | "valueOf" => native("toString", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        }),
        _ => return None,
    };
    Some(method)
}

fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

fn pad_method(at_start: bool) -> Value {
    native("pad", move |_, this, args| {
        let text = this.to_js_string();
        let target = arg(args, 0).to_number();
        let length = text.chars().count();
        if !target.is_finite() || target as usize <= length || target > 1e6 {
            return Ok(Value::string(text));
        }
        let filler = match arg(args, 1) {
            Value::Undefined => " ".to_string(),
            other => other.to_js_string(),
        };
        if filler.is_empty() {
            return Ok(Value::string(text));
        }
        let needed = target as usize - length;
        let padding: String = filler.chars().cycle().take(needed).collect();
        Ok(Value::string(if at_start {
            format!("{padding}{text}")
        } else {
            format!("{text}{padding}")
        }))
    })
}

/// `replace` and `replaceAll`. There is no regular-expression engine, so the
/// pattern is always a plain string; the replacement may be a function.
fn replace_method(all: bool) -> Value {
    native("replace", move |interp, this, args| {
        let text = this.to_js_string();
        let pattern = arg(args, 0).to_js_string();
        let replacement = arg(args, 1);
        if pattern.is_empty() {
            return Ok(Value::string(text));
        }

        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        let mut consumed = 0usize;
        loop {
            let Some(offset) = rest.find(&pattern) else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..offset]);
            let position = consumed + offset;
            if replacement.is_callable() {
                let result = interp.call(
                    &replacement,
                    Value::Undefined,
                    &[
                        Value::string(pattern.clone()),
                        Value::Number(text[..position].chars().count() as f64),
                        Value::string(text.clone()),
                    ],
                )?;
                out.push_str(&result.to_js_string());
            } else {
                out.push_str(&expand_replacement(&replacement.to_js_string(), &pattern));
            }
            let advance = offset + pattern.len();
            rest = &rest[advance..];
            consumed += advance;
            if !all {
                out.push_str(rest);
                break;
            }
        }
        Ok(Value::string(out))
    })
}

/// Expands the `$&` and `$$` patterns a replacement string may contain.
fn expand_replacement(replacement: &str, matched: &str) -> String {
    if !replacement.contains('$') {
        return replacement.to_string();
    }
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('&') => {
                chars.next();
                out.push_str(matched);
            }
            Some('$') => {
                chars.next();
                out.push('$');
            }
            _ => out.push('$'),
        }
    }
    out
}

// ---- array methods --------------------------------------------------------

type ArrayRef = Rc<RefCell<Vec<Value>>>;

fn this_array(interp: &Interp, this: &Value) -> Result<ArrayRef, Control> {
    match this {
        Value::Array(items) => Ok(items.clone()),
        other => Err(interp.type_error(format!("expected an array, got {}", other.type_of()))),
    }
}

/// Reads the callback arguments shared by `map`, `filter`, `forEach` and friends.
fn callback_and_this(interp: &Interp, args: &[Value]) -> Result<(Value, Value), Control> {
    let callback = arg(args, 0);
    if !callback.is_callable() {
        return Err(interp.type_error("expected a function"));
    }
    Ok((callback, arg(args, 1)))
}

/// The methods available on an array.
pub fn array_member(key: &str) -> Option<Value> {
    let method = match key {
        "push" => native("push", |interp, this, args| {
            let items = this_array(interp, this)?;
            let mut items = items.borrow_mut();
            items.extend(args.iter().cloned());
            Ok(Value::Number(items.len() as f64))
        }),
        "pop" => native("pop", |interp, this, _| {
            let items = this_array(interp, this)?;
            let last = items.borrow_mut().pop();
            Ok(last.unwrap_or(Value::Undefined))
        }),
        "shift" => native("shift", |interp, this, _| {
            let items = this_array(interp, this)?;
            let mut items = items.borrow_mut();
            if items.is_empty() {
                return Ok(Value::Undefined);
            }
            Ok(items.remove(0))
        }),
        "unshift" => native("unshift", |interp, this, args| {
            let items = this_array(interp, this)?;
            let mut items = items.borrow_mut();
            for (offset, value) in args.iter().enumerate() {
                items.insert(offset, value.clone());
            }
            Ok(Value::Number(items.len() as f64))
        }),
        "slice" => native("slice", |interp, this, args| {
            let items = this_array(interp, this)?;
            let items = items.borrow();
            let start = relative_index(&arg(args, 0), items.len(), 0);
            let end = relative_index(&arg(args, 1), items.len(), items.len());
            Ok(Value::array(if start >= end {
                Vec::new()
            } else {
                items[start..end].to_vec()
            }))
        }),
        "splice" => native("splice", |interp, this, args| {
            let items = this_array(interp, this)?;
            let mut items = items.borrow_mut();
            let start = relative_index(&arg(args, 0), items.len(), 0);
            let count = match arg(args, 1) {
                Value::Undefined => items.len() - start,
                other => (other.to_number().max(0.0) as usize).min(items.len() - start),
            };
            let removed: Vec<Value> = items
                .splice(start..start + count, args.iter().skip(2).cloned())
                .collect();
            Ok(Value::array(removed))
        }),
        "concat" => native("concat", |interp, this, args| {
            let items = this_array(interp, this)?;
            let mut result = items.borrow().clone();
            for value in args {
                match value {
                    Value::Array(other) => result.extend(other.borrow().iter().cloned()),
                    other => result.push(other.clone()),
                }
            }
            Ok(Value::array(result))
        }),
        "join" => native("join", |interp, this, args| {
            let items = this_array(interp, this)?;
            let separator = match arg(args, 0) {
                Value::Undefined => ",".to_string(),
                other => other.to_js_string(),
            };
            let text = items
                .borrow()
                .iter()
                .map(|item| {
                    if item.is_nullish() {
                        String::new()
                    } else {
                        item.to_js_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(&separator);
            Ok(Value::string(text))
        }),
        "indexOf" => native("indexOf", |interp, this, args| {
            let items = this_array(interp, this)?;
            let needle = arg(args, 0);
            let found = items
                .borrow()
                .iter()
                .position(|item| item.strict_equals(&needle));
            Ok(Value::Number(match found {
                Some(index) => index as f64,
                None => -1.0,
            }))
        }),
        "lastIndexOf" => native("lastIndexOf", |interp, this, args| {
            let items = this_array(interp, this)?;
            let needle = arg(args, 0);
            let found = items
                .borrow()
                .iter()
                .rposition(|item| item.strict_equals(&needle));
            Ok(Value::Number(match found {
                Some(index) => index as f64,
                None => -1.0,
            }))
        }),
        "includes" => native("includes", |interp, this, args| {
            let items = this_array(interp, this)?;
            let needle = arg(args, 0);
            // Unlike `indexOf`, `includes` finds NaN.
            let nan_needle = matches!(&needle, Value::Number(number) if number.is_nan());
            let found = items.borrow().iter().any(|item| {
                item.strict_equals(&needle)
                    || (nan_needle && matches!(item, Value::Number(number) if number.is_nan()))
            });
            Ok(Value::Bool(found))
        }),
        "at" => native("at", |interp, this, args| {
            let items = this_array(interp, this)?;
            let items = items.borrow();
            let index = arg(args, 0).to_number();
            let resolved = if index < 0.0 {
                items.len() as f64 + index
            } else {
                index
            };
            if resolved < 0.0 || resolved as usize >= items.len() {
                return Ok(Value::Undefined);
            }
            Ok(items[resolved as usize].clone())
        }),
        "forEach" => native("forEach", |interp, this, args| {
            let (callback, receiver) = callback_and_this(interp, args)?;
            for (index, item) in snapshot(interp, this)?.into_iter().enumerate() {
                interp.call(
                    &callback,
                    receiver.clone(),
                    &[item, Value::Number(index as f64), this.clone()],
                )?;
            }
            Ok(Value::Undefined)
        }),
        "map" => native("map", |interp, this, args| {
            let (callback, receiver) = callback_and_this(interp, args)?;
            let items = snapshot(interp, this)?;
            let mut mapped = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                mapped.push(interp.call(
                    &callback,
                    receiver.clone(),
                    &[item, Value::Number(index as f64), this.clone()],
                )?);
            }
            Ok(Value::array(mapped))
        }),
        "filter" => native("filter", |interp, this, args| {
            let (callback, receiver) = callback_and_this(interp, args)?;
            let mut kept = Vec::new();
            for (index, item) in snapshot(interp, this)?.into_iter().enumerate() {
                let keep = interp.call(
                    &callback,
                    receiver.clone(),
                    &[item.clone(), Value::Number(index as f64), this.clone()],
                )?;
                if keep.truthy() {
                    kept.push(item);
                }
            }
            Ok(Value::array(kept))
        }),
        "find" => find_method(false, false),
        "findIndex" => find_method(true, false),
        "findLast" => find_method(false, true),
        "findLastIndex" => find_method(true, true),
        "some" => predicate_method(true),
        "every" => predicate_method(false),
        "reduce" => reduce_method(false),
        "reduceRight" => reduce_method(true),
        "reverse" => native("reverse", |interp, this, _| {
            let items = this_array(interp, this)?;
            items.borrow_mut().reverse();
            Ok(this.clone())
        }),
        "sort" => native("sort", |interp, this, args| {
            let comparator = match arg(args, 0) {
                value if value.is_callable() => Some(value),
                Value::Undefined => None,
                other => {
                    return Err(interp
                        .type_error(format!("sort expects a function, got {}", other.type_of())))
                }
            };
            let items = this_array(interp, this)?;
            let sorted = merge_sort(interp, items.borrow().clone(), &comparator)?;
            *items.borrow_mut() = sorted;
            Ok(this.clone())
        }),
        "flat" => native("flat", |interp, this, args| {
            let depth = match arg(args, 0) {
                Value::Undefined => 1.0,
                other => other.to_number(),
            };
            let items = snapshot(interp, this)?;
            Ok(Value::array(flatten(items, depth)))
        }),
        "flatMap" => native("flatMap", |interp, this, args| {
            let (callback, receiver) = callback_and_this(interp, args)?;
            let items = snapshot(interp, this)?;
            let mut mapped = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                mapped.push(interp.call(
                    &callback,
                    receiver.clone(),
                    &[item, Value::Number(index as f64), this.clone()],
                )?);
            }
            Ok(Value::array(flatten(mapped, 1.0)))
        }),
        "fill" => native("fill", |interp, this, args| {
            let items = this_array(interp, this)?;
            let mut items = items.borrow_mut();
            let length = items.len();
            let value = arg(args, 0);
            let start = relative_index(&arg(args, 1), length, 0);
            let end = relative_index(&arg(args, 2), length, length);
            for slot in items.iter_mut().take(end).skip(start) {
                *slot = value.clone();
            }
            drop(items);
            Ok(this.clone())
        }),
        "keys" => native("keys", |interp, this, _| {
            let items = this_array(interp, this)?;
            let length = items.borrow().len();
            Ok(Value::array(
                (0..length)
                    .map(|index| Value::Number(index as f64))
                    .collect(),
            ))
        }),
        "values" => native("values", |interp, this, _| {
            Ok(Value::array(snapshot(interp, this)?))
        }),
        "entries" => native("entries", |interp, this, _| {
            let entries = snapshot(interp, this)?
                .into_iter()
                .enumerate()
                .map(|(index, item)| Value::array(vec![Value::Number(index as f64), item]))
                .collect();
            Ok(Value::array(entries))
        }),
        "toString" | "join_" => native("toString", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        }),
        _ => return None,
    };
    Some(method)
}

/// A copy of the array's contents, taken before any callback runs so the
/// callback is free to mutate the original.
fn snapshot(interp: &Interp, this: &Value) -> Result<Vec<Value>, Control> {
    Ok(this_array(interp, this)?.borrow().clone())
}

fn find_method(want_index: bool, from_end: bool) -> Value {
    native("find", move |interp, this, args| {
        let (callback, receiver) = callback_and_this(interp, args)?;
        let items = snapshot(interp, this)?;
        let order: Vec<usize> = if from_end {
            (0..items.len()).rev().collect()
        } else {
            (0..items.len()).collect()
        };
        for index in order {
            let item = items[index].clone();
            let matched = interp.call(
                &callback,
                receiver.clone(),
                &[item.clone(), Value::Number(index as f64), this.clone()],
            )?;
            if matched.truthy() {
                return Ok(if want_index {
                    Value::Number(index as f64)
                } else {
                    item
                });
            }
        }
        Ok(if want_index {
            Value::Number(-1.0)
        } else {
            Value::Undefined
        })
    })
}

fn predicate_method(any: bool) -> Value {
    native("some", move |interp, this, args| {
        let (callback, receiver) = callback_and_this(interp, args)?;
        for (index, item) in snapshot(interp, this)?.into_iter().enumerate() {
            let matched = interp
                .call(
                    &callback,
                    receiver.clone(),
                    &[item, Value::Number(index as f64), this.clone()],
                )?
                .truthy();
            if matched == any {
                return Ok(Value::Bool(any));
            }
        }
        Ok(Value::Bool(!any))
    })
}

fn reduce_method(from_end: bool) -> Value {
    native("reduce", move |interp, this, args| {
        let callback = arg(args, 0);
        if !callback.is_callable() {
            return Err(interp.type_error("reduce expects a function"));
        }
        let mut items = snapshot(interp, this)?;
        if from_end {
            items.reverse();
        }
        let mut iterator = items.into_iter().enumerate();
        let mut accumulator = match args.get(1) {
            Some(initial) => initial.clone(),
            None => match iterator.next() {
                Some((_, first)) => first,
                None => {
                    return Err(interp.type_error("reduce of empty array with no initial value"))
                }
            },
        };
        for (index, item) in iterator {
            accumulator = interp.call(
                &callback,
                Value::Undefined,
                &[accumulator, item, Value::Number(index as f64), this.clone()],
            )?;
        }
        Ok(accumulator)
    })
}

fn flatten(items: Vec<Value>, depth: f64) -> Vec<Value> {
    if depth < 1.0 {
        return items;
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Array(inner) => {
                out.extend(flatten(inner.borrow().clone(), depth - 1.0));
            }
            other => out.push(other),
        }
    }
    out
}

/// A merge sort, because a comparator can throw and `sort_by` cannot carry an
/// error out. It is also stable, which `Array.prototype.sort` requires.
fn merge_sort(
    interp: &mut Interp,
    items: Vec<Value>,
    comparator: &Option<Value>,
) -> Result<Vec<Value>, Control> {
    if items.len() <= 1 {
        return Ok(items);
    }
    let middle = items.len() / 2;
    let mut left = items;
    let right = left.split_off(middle);
    let left = merge_sort(interp, left, comparator)?;
    let right = merge_sort(interp, right, comparator)?;

    let mut merged = Vec::with_capacity(left.len() + right.len());
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    while left.peek().is_some() && right.peek().is_some() {
        let take_left = {
            let a = left.peek().unwrap();
            let b = right.peek().unwrap();
            compare_for_sort(interp, a, b, comparator)? <= 0.0
        };
        merged.push(if take_left {
            left.next().unwrap()
        } else {
            right.next().unwrap()
        });
    }
    merged.extend(left);
    merged.extend(right);
    Ok(merged)
}

fn compare_for_sort(
    interp: &mut Interp,
    a: &Value,
    b: &Value,
    comparator: &Option<Value>,
) -> Result<f64, Control> {
    // `undefined` sorts last regardless of the comparator.
    match (a, b) {
        (Value::Undefined, Value::Undefined) => return Ok(0.0),
        (Value::Undefined, _) => return Ok(1.0),
        (_, Value::Undefined) => return Ok(-1.0),
        _ => {}
    }
    match comparator {
        Some(function) => {
            let result = interp
                .call(function, Value::Undefined, &[a.clone(), b.clone()])?
                .to_number();
            Ok(if result.is_nan() { 0.0 } else { result })
        }
        // The default comparison is textual, which is what JavaScript does.
        None => Ok(match a.to_js_string().cmp(&b.to_js_string()) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }),
    }
}

// ---- number, boolean, object and function methods -------------------------

/// The methods available on a number.
pub fn number_member(key: &str) -> Option<Value> {
    let method = match key {
        "toFixed" => native("toFixed", |interp, this, args| {
            let digits = arg(args, 0).to_number();
            if !(0.0..=100.0).contains(&digits) && !digits.is_nan() {
                return Err(interp.range_error("toFixed digits must be between 0 and 100"));
            }
            let digits = if digits.is_nan() { 0 } else { digits as usize };
            let number = this.to_number();
            if !number.is_finite() {
                return Ok(Value::string(format_number(number)));
            }
            Ok(Value::string(format!("{number:.digits$}")))
        }),
        "toPrecision" => native("toPrecision", |_, this, args| {
            let number = this.to_number();
            Ok(match arg(args, 0) {
                Value::Undefined => Value::string(format_number(number)),
                digits => {
                    let digits = digits.to_number().clamp(1.0, 21.0) as usize;
                    Value::string(format!("{number:.*e}", digits - 1))
                        .to_js_string()
                        .parse::<f64>()
                        .map(|rounded| Value::string(format_number(rounded)))
                        .unwrap_or(Value::string(format_number(number)))
                }
            })
        }),
        "toString" => native("toString", |interp, this, args| {
            let number = this.to_number();
            Ok(match arg(args, 0) {
                Value::Undefined => Value::string(format_number(number)),
                radix => {
                    let radix = radix.to_number();
                    if !(2.0..=36.0).contains(&radix) {
                        return Err(interp.range_error("toString radix must be between 2 and 36"));
                    }
                    Value::string(to_radix(number, radix as u32))
                }
            })
        }),
        "toLocaleString" => native("toLocaleString", |_, this, _| {
            Ok(Value::string(group_thousands(this.to_number())))
        }),
        "valueOf" => native("valueOf", |_, this, _| Ok(Value::Number(this.to_number()))),
        _ => return None,
    };
    Some(method)
}

/// Formats a number in a base other than ten.
fn to_radix(number: f64, radix: u32) -> String {
    if !number.is_finite() {
        return format_number(number);
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let negative = number < 0.0;
    let mut integer = number.abs().trunc() as u64;
    let mut fraction = number.abs().fract();

    let mut digits = Vec::new();
    if integer == 0 {
        digits.push(b'0');
    }
    while integer > 0 {
        digits.push(DIGITS[(integer % radix as u64) as usize]);
        integer /= radix as u64;
    }
    digits.reverse();
    let mut out = String::from_utf8(digits).unwrap_or_default();

    if fraction > 0.0 {
        out.push('.');
        // Twenty digits is past the precision of an f64 in any base.
        for _ in 0..20 {
            if fraction == 0.0 {
                break;
            }
            fraction *= radix as f64;
            let digit = fraction.trunc() as usize;
            out.push(DIGITS[digit.min(radix as usize - 1)] as char);
            fraction -= fraction.trunc();
        }
    }
    if negative {
        return format!("-{out}");
    }
    out
}

fn group_thousands(number: f64) -> String {
    let text = format_number(number);
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (rest, None),
    };
    if !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return text;
    }
    let mut grouped = String::new();
    for (offset, digit) in integer.chars().enumerate() {
        if offset > 0 && (integer.len() - offset) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped}.{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}

/// The methods available on a boolean.
pub fn boolean_member(key: &str) -> Option<Value> {
    match key {
        "toString" => Some(native("toString", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        })),
        "valueOf" => Some(native("valueOf", |_, this, _| {
            Ok(Value::Bool(this.truthy()))
        })),
        _ => None,
    }
}

/// The methods every object has, reached only after its own properties and its
/// prototype chain have been searched.
pub fn object_member(key: &str) -> Option<Value> {
    let method = match key {
        "hasOwnProperty" => native("hasOwnProperty", |_, this, args| {
            Ok(Value::Bool(has_own(this, &arg(args, 0).to_property_key())))
        }),
        "propertyIsEnumerable" => native("propertyIsEnumerable", |_, this, args| {
            Ok(Value::Bool(has_own(this, &arg(args, 0).to_property_key())))
        }),
        "isPrototypeOf" => native("isPrototypeOf", |_, this, args| {
            let Value::Object(prototype) = this else {
                return Ok(Value::Bool(false));
            };
            let mut current = match arg(args, 0) {
                Value::Object(object) => object.prototype(),
                _ => None,
            };
            for _ in 0..64 {
                let Some(object) = current else { break };
                if Rc::ptr_eq(&object, prototype) {
                    return Ok(Value::Bool(true));
                }
                current = object.prototype();
            }
            Ok(Value::Bool(false))
        }),
        "toString" | "toLocaleString" => native("toString", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        }),
        "valueOf" => native("valueOf", |_, this, _| Ok(this.clone())),
        _ => return None,
    };
    Some(method)
}

/// The methods every function has.
pub fn function_member(key: &str) -> Option<Value> {
    let method = match key {
        "call" => native("call", |interp, this, args| {
            let target = this.clone();
            interp.call(
                &target,
                arg(args, 0),
                &args.iter().skip(1).cloned().collect::<Vec<_>>(),
            )
        }),
        "apply" => native("apply", |interp, this, args| {
            let target = this.clone();
            let list = match arg(args, 1) {
                Value::Undefined | Value::Null => Vec::new(),
                other => interp.iterate(&other)?,
            };
            interp.call(&target, arg(args, 0), &list)
        }),
        "bind" => native("bind", |interp, this, args| {
            if !this.is_callable() {
                return Err(interp.type_error("bind expects a function"));
            }
            let target = this.clone();
            let receiver = arg(args, 0);
            let bound: Vec<Value> = args.iter().skip(1).cloned().collect();
            // A bound function is a fresh native that forwards, which handles
            // partial application uniformly for closures and natives alike.
            Ok(native("bound", move |interp, _this, call_args| {
                let mut all = bound.clone();
                all.extend(call_args.iter().cloned());
                interp.call(&target, receiver.clone(), &all)
            }))
        }),
        "toString" => native("toString", |_, this, _| {
            Ok(Value::string(this.to_js_string()))
        }),
        _ => return None,
    };
    Some(method)
}

// ---- number parsing and URI escaping --------------------------------------

/// `parseInt`, which reads a leading integer and ignores whatever follows.
fn parse_int(text: &str, radix: Value) -> f64 {
    let text = text.trim();
    let mut chars = text.chars().peekable();
    let mut negative = false;
    match chars.peek() {
        Some('+') => {
            chars.next();
        }
        Some('-') => {
            chars.next();
            negative = true;
        }
        _ => {}
    }

    let mut rest: String = chars.collect();
    let mut base = match radix {
        Value::Undefined => 0,
        other => {
            let value = other.to_number();
            if !value.is_finite() {
                0
            } else {
                value as u32
            }
        }
    };
    if base == 0 {
        if rest.starts_with("0x") || rest.starts_with("0X") {
            base = 16;
            rest = rest[2..].to_string();
        } else {
            base = 10;
        }
    } else if base == 16 && (rest.starts_with("0x") || rest.starts_with("0X")) {
        rest = rest[2..].to_string();
    }
    if !(2..=36).contains(&base) {
        return f64::NAN;
    }

    let mut value = 0.0f64;
    let mut digits = 0;
    for ch in rest.chars() {
        match ch.to_digit(36) {
            Some(digit) if digit < base => {
                value = value * base as f64 + digit as f64;
                digits += 1;
            }
            _ => break,
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if negative {
        -value
    } else {
        value
    }
}

/// `parseFloat`, which reads the longest leading decimal number.
fn parse_float(text: &str) -> f64 {
    let text = text.trim();
    if text.starts_with("Infinity") || text.starts_with("+Infinity") {
        return f64::INFINITY;
    }
    if text.starts_with("-Infinity") {
        return f64::NEG_INFINITY;
    }
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_exponent = false;
    let mut seen_digit = false;
    for (offset, ch) in text.char_indices() {
        let accept = match ch {
            '+' | '-' => {
                offset == 0 || matches!(text[..offset].chars().next_back(), Some('e') | Some('E'))
            }
            '.' => !seen_dot && !seen_exponent,
            'e' | 'E' => seen_digit && !seen_exponent,
            '0'..='9' => true,
            _ => false,
        };
        if !accept {
            break;
        }
        match ch {
            '.' => seen_dot = true,
            'e' | 'E' => seen_exponent = true,
            '0'..='9' => seen_digit = true,
            _ => {}
        }
        end = offset + ch.len_utf8();
    }
    if !seen_digit {
        return f64::NAN;
    }
    // Trailing exponent markers make the prefix unparseable, so back off.
    let mut candidate = &text[..end];
    while !candidate.is_empty() && candidate.parse::<f64>().is_err() {
        candidate = &candidate[..candidate.len() - 1];
    }
    candidate.parse().unwrap_or(f64::NAN)
}

const COMPONENT_SAFE: &str = "-_.!~*'()";
const URI_SAFE: &str = "-_.!~*'();/?:@&=+$,#";

fn encode_uri(text: &str, safe: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || safe.contains(ch) {
            out.push(ch);
            continue;
        }
        let mut buffer = [0u8; 4];
        for byte in ch.encode_utf8(&mut buffer).as_bytes() {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode_uri(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = (bytes[index + 1] as char).to_digit(16)?;
            let low = (bytes[index + 2] as char).to_digit(16)?;
            out.push((high * 16 + low) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> String {
        let mut interp = Interp::new();
        match interp.eval(source) {
            Ok(value) => value.to_js_string(),
            Err(message) => panic!("{source} failed: {message}"),
        }
    }

    fn logged(source: &str) -> Vec<String> {
        let mut interp = Interp::new();
        interp.eval(source).expect("script should run");
        interp
            .console
            .iter()
            .map(|message| message.text.clone())
            .collect()
    }

    #[test]
    fn console_captures_messages_with_levels() {
        let mut interp = Interp::new();
        interp
            .eval("console.log('a', 1); console.warn('careful'); console.error('bad')")
            .unwrap();
        assert_eq!(interp.console.len(), 3);
        assert_eq!(interp.console[0].text, "a 1");
        assert_eq!(interp.console[0].level, ConsoleLevel::Log);
        assert_eq!(interp.console[1].level, ConsoleLevel::Warn);
        assert_eq!(interp.console[2].level, ConsoleLevel::Error);
    }

    #[test]
    fn math_functions() {
        assert_eq!(run("Math.max(1, 7, 3)"), "7");
        assert_eq!(run("Math.min()"), "Infinity");
        assert_eq!(run("Math.round(2.5)"), "3");
        assert_eq!(run("Math.round(-2.5)"), "-2", "halves go towards +Infinity");
        assert_eq!(run("Math.abs(-4)"), "4");
        assert_eq!(run("Math.pow(2, 10)"), "1024");
        assert_eq!(run("Math.sign(-3)"), "-1");
        assert_eq!(run("Math.floor(Math.PI)"), "3");
        assert_eq!(run("Math.hypot(3, 4)"), "5");
    }

    #[test]
    fn math_random_stays_in_range() {
        assert_eq!(
            run("let ok = true; for (let i = 0; i < 200; i++) { const r = Math.random(); if (r < 0 || r >= 1) ok = false; } ok"),
            "true"
        );
    }

    #[test]
    fn string_methods() {
        assert_eq!(run("'hello'.toUpperCase()"), "HELLO");
        assert_eq!(run("'  pad '.trim()"), "pad");
        assert_eq!(run("'a-b-c'.split('-').length"), "3");
        assert_eq!(run("'a-b-c'.split('-')[1]"), "b");
        assert_eq!(run("'hello'.slice(1, 3)"), "el");
        assert_eq!(run("'hello'.slice(-3)"), "llo");
        assert_eq!(run("'hello'.indexOf('l')"), "2");
        assert_eq!(run("'hello'.charAt(1)"), "e");
        assert_eq!(run("'hello'.charCodeAt(0)"), "104");
        assert_eq!(run("'hi'.repeat(3)"), "hihihi");
        assert_eq!(run("'7'.padStart(3, '0')"), "007");
        assert_eq!(run("'a'.padEnd(4, '.')"), "a...");
        assert_eq!(run("'hello'.startsWith('he')"), "true");
        assert_eq!(run("'hello'.endsWith('lo')"), "true");
        assert_eq!(run("'hello'.includes('ell')"), "true");
        assert_eq!(run("'hello'.at(-1)"), "o");
        assert_eq!(run("'abc'.substring(2, 0)"), "ab", "substring swaps");
        assert_eq!(run("'abc'.slice(2, 0)"), "", "slice does not swap");
    }

    #[test]
    fn string_replacement() {
        assert_eq!(run("'a-b-c'.replace('-', '+')"), "a+b-c");
        assert_eq!(run("'a-b-c'.replaceAll('-', '+')"), "a+b+c");
        assert_eq!(run("'ab'.replace('b', '[$&]')"), "a[b]");
        assert_eq!(
            run("'a1b1'.replaceAll('1', (m, i) => i)"),
            "a1b3",
            "the callback receives the match index"
        );
    }

    #[test]
    fn unicode_strings_index_by_character() {
        assert_eq!(run("'héllo'.length"), "5");
        assert_eq!(run("'héllo'.charAt(1)"), "é");
        assert_eq!(run("'héllo'.indexOf('l')"), "2");
        assert_eq!(run("'héllo'.slice(1, 3)"), "él");
    }

    #[test]
    fn array_mutation() {
        assert_eq!(run("const a = [1]; a.push(2, 3); a.join('-')"), "1-2-3");
        assert_eq!(run("const a = [1, 2]; a.pop(); a.length"), "1");
        assert_eq!(run("const a = [1, 2]; a.shift()"), "1");
        assert_eq!(run("const a = [2]; a.unshift(1); a.toString()"), "1,2");
        assert_eq!(
            run("const a = [1, 2, 3]; a.splice(1, 1); a.toString()"),
            "1,3"
        );
        assert_eq!(
            run("const a = [1, 2, 3]; a.splice(1, 1, 9, 9); a.toString()"),
            "1,9,9,3"
        );
        assert_eq!(run("[1, 2, 3].reverse().toString()"), "3,2,1");
        assert_eq!(run("[1, 2, 3, 4].fill(0, 1, 3).toString()"), "1,0,0,4");
    }

    #[test]
    fn array_iteration() {
        assert_eq!(run("[1, 2, 3].map(n => n * 2).toString()"), "2,4,6");
        assert_eq!(
            run("[1, 2, 3, 4].filter(n => n % 2 === 0).toString()"),
            "2,4"
        );
        assert_eq!(run("[1, 2, 3].reduce((a, b) => a + b)"), "6");
        assert_eq!(run("[1, 2, 3].reduce((a, b) => a + b, 10)"), "16");
        assert_eq!(run("[1, 2, 3].find(n => n > 1)"), "2");
        assert_eq!(run("[1, 2, 3].findIndex(n => n > 1)"), "1");
        assert_eq!(run("[1, 2, 3].findLast(n => n < 3)"), "2");
        assert_eq!(run("[1, 2, 3].some(n => n > 2)"), "true");
        assert_eq!(run("[1, 2, 3].every(n => n > 2)"), "false");
        assert_eq!(run("[[1], [2, 3]].flat().toString()"), "1,2,3");
        assert_eq!(
            run("Array.isArray([[1, [2]]].flat()[1])"),
            "true",
            "flat only descends one level by default"
        );
        assert_eq!(run("[[1, [2]]].flat(2).toString()"), "1,2");
        assert_eq!(run("[1, 2].flatMap(n => [n, n]).toString()"), "1,1,2,2");
        assert_eq!(
            run("let total = 0; [1, 2, 3].forEach(n => { total += n; }); total"),
            "6"
        );
    }

    #[test]
    fn array_sorting_is_stable_and_uses_the_comparator() {
        assert_eq!(
            run("[10, 9, 1].sort().toString()"),
            "1,10,9",
            "default is textual"
        );
        assert_eq!(run("[10, 9, 1].sort((a, b) => a - b).toString()"), "1,9,10");
        assert_eq!(
            run("[3, 1, 2, undefined].sort((a, b) => a - b).toString()"),
            "1,2,3,",
            "undefined sorts last"
        );
        // Stability: equal keys keep their original relative order.
        assert_eq!(
            run("[{k:1,v:'a'},{k:0,v:'b'},{k:1,v:'c'}].sort((x, y) => x.k - y.k).map(e => e.v).join('')"),
            "bac"
        );
    }

    #[test]
    fn a_callback_may_mutate_the_array_it_is_iterating() {
        // The snapshot taken before the callback runs is what keeps this from
        // panicking on a double borrow.
        assert_eq!(
            run("const a = [1, 2, 3]; const out = a.map(n => { a.push(n); return n; }); out.toString()"),
            "1,2,3"
        );
    }

    #[test]
    fn array_statics() {
        assert_eq!(run("Array.isArray([])"), "true");
        assert_eq!(run("Array.isArray('no')"), "false");
        assert_eq!(run("Array.of(1, 2).toString()"), "1,2");
        assert_eq!(run("Array.from('abc').toString()"), "a,b,c");
        assert_eq!(run("Array.from([1, 2], n => n * 3).toString()"), "3,6");
        assert_eq!(run("new Array(3).length"), "3");
    }

    #[test]
    fn object_statics() {
        assert_eq!(run("Object.keys({a: 1, b: 2}).toString()"), "a,b");
        assert_eq!(run("Object.values({a: 1, b: 2}).toString()"), "1,2");
        assert_eq!(run("Object.entries({a: 1})[0].toString()"), "a,1");
        assert_eq!(run("Object.assign({}, {a: 1}, {b: 2}).b"), "2");
        assert_eq!(run("Object.fromEntries([['a', 1]]).a"), "1");
        assert_eq!(run("Object.hasOwn({a: 1}, 'a')"), "true");
        assert_eq!(run("Object.hasOwn({a: 1}, 'b')"), "false");
        assert_eq!(run("const p = {greet: 1}; Object.create(p).greet"), "1");
    }

    #[test]
    fn object_methods() {
        assert_eq!(run("({a: 1}).hasOwnProperty('a')"), "true");
        assert_eq!(run("({}).toString()"), "[object Object]");
    }

    #[test]
    fn json_round_trip() {
        assert_eq!(
            run("JSON.stringify({a: 1, b: [true, null], c: 'x'})"),
            r#"{"a":1,"b":[true,null],"c":"x"}"#
        );
        assert_eq!(run("JSON.stringify([1, undefined, 2])"), "[1,null,2]");
        assert_eq!(
            run("JSON.stringify({a: undefined, b: 1})"),
            r#"{"b":1}"#,
            "undefined properties are dropped"
        );
        assert_eq!(run("JSON.parse('{\"a\": [1, 2]}').a[1]"), "2");
        assert_eq!(run("JSON.parse('\"tab\\\\there\"')"), "tab\there");
        assert_eq!(run("JSON.parse(JSON.stringify({n: 1.5})).n"), "1.5");
        assert_eq!(
            run("JSON.stringify('he said \"hi\"')"),
            r#""he said \"hi\"""#
        );
    }

    #[test]
    fn json_stringify_indents() {
        assert_eq!(run("JSON.stringify({a: 1}, null, 2)"), "{\n  \"a\": 1\n}");
        assert_eq!(run("JSON.stringify([1], null, 1)"), "[\n 1\n]");
    }

    #[test]
    fn json_parse_rejects_bad_input() {
        let mut interp = Interp::new();
        let error = interp.eval("JSON.parse('{oops}')").unwrap_err();
        assert!(error.contains("JSON"), "{error}");
    }

    #[test]
    fn number_methods() {
        assert_eq!(run("(1.005).toFixed(2)"), "1.00");
        assert_eq!(run("(3.14159).toFixed(2)"), "3.14");
        assert_eq!(run("(255).toString(16)"), "ff");
        assert_eq!(run("(5).toString(2)"), "101");
        assert_eq!(run("(-10).toString(2)"), "-1010");
        assert_eq!(run("(1234567.5).toLocaleString()"), "1,234,567.5");
        assert_eq!(run("(42).valueOf()"), "42");
    }

    #[test]
    fn number_statics() {
        assert_eq!(run("Number.isInteger(4)"), "true");
        assert_eq!(run("Number.isInteger(4.5)"), "false");
        assert_eq!(run("Number.isInteger('4')"), "false");
        assert_eq!(run("Number.isNaN(NaN)"), "true");
        assert_eq!(
            run("Number.isNaN('x')"),
            "false",
            "no coercion, unlike isNaN"
        );
        assert_eq!(run("isNaN('x')"), "true");
        assert_eq!(run("Number.MAX_SAFE_INTEGER"), "9007199254740991");
        assert_eq!(run("Number('12')"), "12");
    }

    #[test]
    fn parsing_numbers_from_strings() {
        assert_eq!(run("parseInt('42px')"), "42");
        assert_eq!(run("parseInt('-7')"), "-7");
        assert_eq!(run("parseInt('ff', 16)"), "255");
        assert_eq!(run("parseInt('0x1f')"), "31");
        assert_eq!(run("parseInt('nope')"), "NaN");
        assert_eq!(run("parseFloat('3.5rem')"), "3.5");
        assert_eq!(run("parseFloat('1e3')"), "1000");
        assert_eq!(run("parseFloat('.5')"), "0.5");
        assert_eq!(run("parseFloat('-2.5e-2')"), "-0.025");
        assert_eq!(run("parseFloat('abc')"), "NaN");
    }

    #[test]
    fn errors_are_constructed_and_caught() {
        assert_eq!(
            run("try { throw new Error('boom') } catch (e) { e.message }"),
            "boom"
        );
        assert_eq!(
            run("try { throw new TypeError('bad') } catch (e) { e.name }"),
            "TypeError"
        );
        assert_eq!(
            run("try { null.x } catch (e) { e instanceof TypeError }"),
            "true",
            "an interpreter error is a real TypeError"
        );
        assert_eq!(
            run("try { throw new TypeError('x') } catch (e) { e instanceof Error }"),
            "true",
            "every error kind is an Error"
        );
        assert_eq!(
            run("try { throw new RangeError('x') } catch (e) { e instanceof TypeError }"),
            "false"
        );
        assert_eq!(run("String(new Error('boom'))"), "Error: boom");
    }

    #[test]
    fn instance_of_the_native_constructors() {
        assert_eq!(run("[] instanceof Array"), "true");
        assert_eq!(run("({}) instanceof Array"), "false");
        assert_eq!(run("({}) instanceof Object"), "true");
        assert_eq!(run("1 instanceof Object"), "false");
        assert_eq!(run("new Date() instanceof Date"), "true");
    }

    #[test]
    fn function_call_apply_and_bind() {
        assert_eq!(
            run("function f(a, b) { return this.base + a + b } f.call({base: 1}, 2, 3)"),
            "6"
        );
        assert_eq!(
            run("function f(a, b) { return this.base + a + b } f.apply({base: 1}, [2, 3])"),
            "6"
        );
        assert_eq!(
            run("function f(a, b) { return this.base + a + b } const g = f.bind({base: 1}, 2); g(3)"),
            "6",
            "bind supports partial application"
        );
        assert_eq!(run("(function () { return 1 }).name"), "");
        assert_eq!(run("function named() {} named.name"), "named");
    }

    #[test]
    fn functions_carry_properties() {
        assert_eq!(run("function f() {} f.tag = 'x'; f.tag"), "x");
        assert_eq!(run("Math.max.name"), "max");
    }

    #[test]
    fn timers_are_queued_for_the_host() {
        let mut interp = Interp::new();
        interp
            .eval("setTimeout(() => { console.log('later') }, 10)")
            .unwrap();
        let timers = interp.take_timers();
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].delay, 10.0);
        assert!(!timers[0].repeating);
        assert!(interp.console.is_empty(), "the callback has not run yet");

        // Running it is the host's job, and it uses the same public API.
        let callback = timers[0].callback.clone();
        interp.call(&callback, Value::Undefined, &[]).unwrap();
        assert_eq!(interp.console[0].text, "later");
    }

    #[test]
    fn a_cancelled_timer_is_not_delivered() {
        let mut interp = Interp::new();
        interp
            .eval("const id = setTimeout(() => {}, 0); clearTimeout(id)")
            .unwrap();
        assert!(interp.take_timers().is_empty());
    }

    #[test]
    fn uri_escaping() {
        assert_eq!(run("encodeURIComponent('a b&c')"), "a%20b%26c");
        assert_eq!(run("encodeURI('http://x/a b')"), "http://x/a%20b");
        assert_eq!(run("decodeURIComponent('a%20b%26c')"), "a b&c");
        assert_eq!(run("encodeURIComponent('é')"), "%C3%A9");
        assert_eq!(run("decodeURIComponent('%C3%A9')"), "é");
    }

    #[test]
    fn dates_report_utc_fields() {
        assert_eq!(run("new Date(0).getFullYear()"), "1970");
        assert_eq!(run("new Date(0).getMonth()"), "0");
        assert_eq!(run("new Date(0).getDate()"), "1");
        assert_eq!(
            run("new Date(0).getDay()"),
            "4",
            "1970-01-01 was a Thursday"
        );
        assert_eq!(run("new Date(0).toISOString()"), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            run("new Date(1700000000000).toISOString()"),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(
            run("new Date(-1).toISOString()"),
            "1969-12-31T23:59:59.999Z"
        );
        assert_eq!(run("typeof Date.now()"), "number");
    }

    #[test]
    fn console_logging_renders_structures() {
        assert_eq!(logged("console.log([1, 'a'])"), vec!["[ 1, 'a' ]"]);
        assert_eq!(logged("console.log({a: 1})"), vec!["{ a: 1 }"]);
        assert_eq!(logged("console.log('plain')"), vec!["plain"]);
        assert_eq!(
            logged("console.log(null, undefined)"),
            vec!["null undefined"]
        );
    }

    #[test]
    fn the_console_cannot_be_used_to_exhaust_memory() {
        let mut interp = Interp::new();
        interp
            .eval("for (let i = 0; i < 5000; i++) console.log(i)")
            .unwrap();
        assert_eq!(interp.console.len(), 1000);
    }

    #[test]
    fn repeat_refuses_to_allocate_an_enormous_string() {
        let mut interp = Interp::new();
        let error = interp.eval("'x'.repeat(1e9)").unwrap_err();
        assert!(error.contains("too long"), "{error}");
    }
}
