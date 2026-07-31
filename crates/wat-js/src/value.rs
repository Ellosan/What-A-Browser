//! Runtime values, objects, scopes and the conversions between them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::Function;

/// A value the browser's own code exposes to scripts, such as a DOM node.
///
/// The interpreter knows nothing about the DOM; it routes property access and
/// method calls on these handles straight through to the host.
pub trait HostObject {
    /// Used by `typeof`-adjacent diagnostics and `console.log`.
    fn type_name(&self) -> String;

    /// Reads a property. `None` means "no such property".
    fn get(&self, key: &str) -> Option<Value>;

    /// Writes a property, returning whether it was accepted.
    fn set(&self, key: &str, value: &Value) -> bool;

    /// Calls a method. `Err` becomes a thrown JavaScript error.
    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String>;

    /// Enumerable keys, for `for-in` and `Object.keys`.
    fn own_keys(&self) -> Vec<String> {
        Vec::new()
    }

    /// How `console.log` and string conversion render this object.
    fn describe(&self) -> String {
        format!("[object {}]", self.type_name())
    }

    /// A stable identity, so two handles to the same underlying thing compare
    /// equal with `===`.
    fn identity(&self) -> usize;
}

/// A property reached through an object or its prototypes.
pub enum Slot {
    /// A stored value.
    Value(Value),
    /// A getter, which the caller must call with the original receiver.
    Getter(Value),
}

/// A `get`/`set` pair. Either half may be missing.
#[derive(Clone, Default)]
struct Accessor {
    getter: Option<Value>,
    setter: Option<Value>,
}

/// Where a property write lands.
pub enum WriteTarget {
    /// Store the value on the object.
    Store,
    /// Call this setter with the value.
    Setter(Value),
    /// Drop the write: the property has a getter but no setter.
    Ignore,
}

/// A plain JavaScript object: ordered properties plus a prototype.
pub struct JsObject {
    /// Insertion-ordered, which `Object.keys` and `JSON.stringify` rely on.
    properties: RefCell<Vec<(String, Value)>>,
    /// Accessors, kept apart from the stored values so a read can tell the two
    /// cases apart without wrapping every ordinary property.
    accessors: RefCell<Vec<(String, Accessor)>>,
    prototype: RefCell<Option<Rc<JsObject>>>,
    /// The constructor name, used by `console.log` and error messages.
    pub class_name: RefCell<String>,
}

impl std::fmt::Debug for JsObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsObject({})", self.class_name.borrow())
    }
}

impl Default for JsObject {
    fn default() -> Self {
        JsObject::new()
    }
}

impl JsObject {
    pub fn new() -> Self {
        JsObject {
            properties: RefCell::new(Vec::new()),
            accessors: RefCell::new(Vec::new()),
            prototype: RefCell::new(None),
            class_name: RefCell::new("Object".to_string()),
        }
    }

    pub fn with_class(name: impl Into<String>) -> Self {
        let object = JsObject::new();
        *object.class_name.borrow_mut() = name.into();
        object
    }

    pub fn set_prototype(&self, prototype: Option<Rc<JsObject>>) {
        *self.prototype.borrow_mut() = prototype;
    }

    pub fn prototype(&self) -> Option<Rc<JsObject>> {
        self.prototype.borrow().clone()
    }

    /// Reads an own property, ignoring the prototype chain.
    pub fn own(&self, key: &str) -> Option<Value> {
        self.properties
            .borrow()
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    pub fn has_own(&self, key: &str) -> bool {
        self.properties.borrow().iter().any(|(name, _)| name == key)
    }

    /// Reads a property, walking the prototype chain.
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(value) = self.own(key) {
            return Some(value);
        }
        let mut current = self.prototype();
        // The chain is bounded so a cycle cannot hang the interpreter.
        for _ in 0..64 {
            let object = current?;
            if let Some(value) = object.own(key) {
                return Some(value);
            }
            current = object.prototype();
        }
        None
    }

    pub fn set(&self, key: impl Into<String>, value: Value) {
        let key = key.into();
        let mut properties = self.properties.borrow_mut();
        match properties.iter_mut().find(|(name, _)| *name == key) {
            Some(slot) => slot.1 = value,
            None => properties.push((key, value)),
        }
    }

    pub fn delete(&self, key: &str) -> bool {
        let mut properties = self.properties.borrow_mut();
        let before = properties.len();
        properties.retain(|(name, _)| name != key);
        let removed_value = properties.len() != before;
        drop(properties);
        let mut accessors = self.accessors.borrow_mut();
        let before = accessors.len();
        accessors.retain(|(name, _)| name != key);
        removed_value || accessors.len() != before
    }

    /// Defines a getter, keeping any setter already defined for the same key.
    pub fn define_getter(&self, key: impl Into<String>, getter: Value) {
        self.with_accessor(key, |accessor| accessor.getter = Some(getter));
    }

    /// Defines a setter, keeping any getter already defined for the same key.
    pub fn define_setter(&self, key: impl Into<String>, setter: Value) {
        self.with_accessor(key, |accessor| accessor.setter = Some(setter));
    }

    fn with_accessor(&self, key: impl Into<String>, edit: impl FnOnce(&mut Accessor)) {
        let key = key.into();
        let mut accessors = self.accessors.borrow_mut();
        match accessors.iter_mut().find(|(name, _)| *name == key) {
            Some((_, accessor)) => edit(accessor),
            None => {
                let mut accessor = Accessor::default();
                edit(&mut accessor);
                accessors.push((key, accessor));
            }
        }
    }

    fn own_accessor(&self, key: &str) -> Option<Accessor> {
        self.accessors
            .borrow()
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, accessor)| accessor.clone())
    }

    pub fn has_own_accessor(&self, key: &str) -> bool {
        self.accessors.borrow().iter().any(|(name, _)| name == key)
    }

    /// Finds a property on this object or its prototypes, reporting whether it
    /// is stored or computed by a getter.
    pub fn find(&self, key: &str) -> Option<Slot> {
        if let Some(value) = self.own(key) {
            return Some(Slot::Value(value));
        }
        if let Some(accessor) = self.own_accessor(key) {
            return Some(match accessor.getter {
                Some(getter) => Slot::Getter(getter),
                // A set-only property reads as undefined.
                None => Slot::Value(Value::Undefined),
            });
        }
        let mut current = self.prototype();
        for _ in 0..64 {
            let object = current?;
            if let Some(value) = object.own(key) {
                return Some(Slot::Value(value));
            }
            if let Some(accessor) = object.own_accessor(key) {
                return Some(match accessor.getter {
                    Some(getter) => Slot::Getter(getter),
                    None => Slot::Value(Value::Undefined),
                });
            }
            current = object.prototype();
        }
        None
    }

    /// Where a write to `key` should go.
    ///
    /// An own stored property shadows a setter further up the chain, which is
    /// what keeps `this._x = v` inside a `set x` accessor from recursing.
    pub fn write_target(&self, key: &str) -> WriteTarget {
        fn accessor_target(accessor: Accessor) -> WriteTarget {
            match accessor.setter {
                Some(setter) => WriteTarget::Setter(setter),
                None => WriteTarget::Ignore,
            }
        }

        if self.has_own(key) {
            return WriteTarget::Store;
        }
        if let Some(accessor) = self.own_accessor(key) {
            return accessor_target(accessor);
        }
        let mut current = self.prototype();
        for _ in 0..64 {
            let Some(object) = current else { break };
            if object.has_own(key) {
                return WriteTarget::Store;
            }
            if let Some(accessor) = object.own_accessor(key) {
                return accessor_target(accessor);
            }
            current = object.prototype();
        }
        WriteTarget::Store
    }

    /// Enumerable keys. Private class fields are named `#something` and are
    /// deliberately left out, so they stay out of `Object.keys`, `for-in` and
    /// `JSON.stringify`.
    pub fn keys(&self) -> Vec<String> {
        let stored = self.properties.borrow();
        let accessors = self.accessors.borrow();
        stored
            .iter()
            .map(|(name, _)| name)
            .chain(accessors.iter().map(|(name, _)| name))
            .filter(|name| !name.starts_with('#'))
            .cloned()
            .collect()
    }

    pub fn entries(&self) -> Vec<(String, Value)> {
        self.properties.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.properties.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A user-defined function together with the scope it captured.
pub struct Closure {
    pub function: Rc<Function>,
    pub scope: Rc<Scope>,
    /// Bound receiver: arrow functions capture it, `bind` sets it.
    pub this: Option<Value>,
    /// The object `new` gives instances as their prototype.
    pub prototype: Rc<JsObject>,
    /// The prototype of the class this method was defined in, for `super`.
    pub home_prototype: Option<Rc<JsObject>>,
    pub name: String,
    /// Properties hung directly on the function, which is where static class
    /// members and ad-hoc `fn.someFlag = …` assignments live.
    pub properties: Rc<JsObject>,
}

impl std::fmt::Debug for Closure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[Function: {}]", self.name)
    }
}

/// The Rust side of a native function: the interpreter, the receiver and the
/// arguments in, a value or a thrown error out.
pub type NativeBody =
    dyn Fn(&mut crate::interp::Interp, &Value, &[Value]) -> Result<Value, crate::interp::Control>;

/// A function implemented in Rust.
pub struct NativeFunction {
    pub name: &'static str,
    /// Properties hung on the function, which is how `Math.PI`, `Number.isNaN`
    /// and the other namespace-style built-ins are reached.
    pub properties: Rc<JsObject>,
    pub func: Box<NativeBody>,
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[Function: {}]", self.name)
    }
}

/// A JavaScript value.
#[derive(Clone)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(Rc<String>),
    Object(Rc<JsObject>),
    Array(Rc<RefCell<Vec<Value>>>),
    Function(Rc<Closure>),
    Native(Rc<NativeFunction>),
    Host(Rc<dyn HostObject>),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&inspect(self))
    }
}

impl Value {
    pub fn string(text: impl Into<String>) -> Value {
        Value::Str(Rc::new(text.into()))
    }

    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    pub fn object(object: JsObject) -> Value {
        Value::Object(Rc::new(object))
    }

    /// The result of `typeof`.
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Str(_) => "string",
            Value::Function(_) | Value::Native(_) => "function",
            Value::Object(_) | Value::Array(_) | Value::Host(_) => "object",
        }
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    pub fn is_callable(&self) -> bool {
        matches!(self, Value::Function(_) | Value::Native(_))
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(number) => *number != 0.0 && !number.is_nan(),
            Value::Str(text) => !text.is_empty(),
            _ => true,
        }
    }

    /// `ToNumber`.
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(true) => 1.0,
            Value::Bool(false) => 0.0,
            Value::Number(number) => *number,
            Value::Str(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    0.0
                } else if let Some(hex) = trimmed.strip_prefix("0x").or(trimmed.strip_prefix("0X"))
                {
                    i64::from_str_radix(hex, 16)
                        .map(|v| v as f64)
                        .unwrap_or(f64::NAN)
                } else if trimmed == "Infinity" || trimmed == "+Infinity" {
                    f64::INFINITY
                } else if trimmed == "-Infinity" {
                    f64::NEG_INFINITY
                } else {
                    trimmed.parse().unwrap_or(f64::NAN)
                }
            }
            // An empty array converts to 0, a one-element array to its element.
            Value::Array(items) => {
                let items = items.borrow();
                match items.len() {
                    0 => 0.0,
                    1 => items[0].to_number(),
                    _ => f64::NAN,
                }
            }
            _ => f64::NAN,
        }
    }

    /// `ToInt32`, for the bitwise operators.
    pub fn to_int32(&self) -> i32 {
        let number = self.to_number();
        if !number.is_finite() {
            return 0;
        }
        (number.trunc() as i64 & 0xffff_ffff) as u32 as i32
    }

    pub fn to_uint32(&self) -> u32 {
        self.to_int32() as u32
    }

    /// `ToString`.
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            Value::Number(number) => format_number(*number),
            Value::Str(text) => text.as_str().to_string(),
            Value::Array(items) => items
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
                .join(","),
            Value::Function(closure) => format!("function {}() {{ … }}", closure.name),
            Value::Native(native) => format!("function {}() {{ [native code] }}", native.name),
            Value::Host(host) => host.describe(),
            Value::Object(object) => {
                // An object with a `toString` method uses it, as errors do.
                if let Some(message) = object.own("message") {
                    let name = object
                        .own("name")
                        .map(|value| value.to_js_string())
                        .unwrap_or_else(|| object.class_name.borrow().clone());
                    let message = message.to_js_string();
                    if message.is_empty() {
                        return name;
                    }
                    return format!("{name}: {message}");
                }
                "[object Object]".to_string()
            }
        }
    }

    /// A property key, as used by member access.
    pub fn to_property_key(&self) -> String {
        self.to_js_string()
    }

    /// `===`
    pub fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
            (Value::Host(a), Value::Host(b)) => a.identity() == b.identity(),
            _ => false,
        }
    }

    /// `==`
    pub fn loose_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
            (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => false,
            (Value::Number(_) | Value::Bool(_), Value::Str(_))
            | (Value::Str(_), Value::Number(_) | Value::Bool(_))
            | (Value::Bool(_), Value::Number(_))
            | (Value::Number(_), Value::Bool(_)) => {
                let a = self.to_number();
                let b = other.to_number();
                a == b
            }
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            // Comparing an object with a primitive converts the object.
            (
                Value::Object(_) | Value::Array(_) | Value::Host(_),
                Value::Number(_) | Value::Str(_),
            ) => {
                self.to_js_string() == other.to_js_string() || self.to_number() == other.to_number()
            }
            (
                Value::Number(_) | Value::Str(_),
                Value::Object(_) | Value::Array(_) | Value::Host(_),
            ) => other.loose_equals(self),
            _ => self.strict_equals(other),
        }
    }

    /// The `length` of a string or array, if it has one.
    pub fn length(&self) -> Option<usize> {
        match self {
            Value::Str(text) => Some(text.chars().count()),
            Value::Array(items) => Some(items.borrow().len()),
            _ => None,
        }
    }
}

/// Formats a number the way JavaScript does.
pub fn format_number(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if number == 0.0 {
        // Both zeroes print as "0".
        return "0".to_string();
    }
    // JavaScript switches to exponential notation outside this range, while
    // Rust's `Display` never does.
    let magnitude = number.abs();
    if !(1e-6..1e21).contains(&magnitude) {
        let text = format!("{number:e}");
        // Rust writes exponents as `1e21`; JavaScript writes `1e+21`.
        return match text.split_once('e') {
            Some((mantissa, exponent)) if !exponent.starts_with('-') => {
                format!("{mantissa}e+{exponent}")
            }
            _ => text,
        };
    }
    if number.fract() == 0.0 {
        // `as i64` would saturate above 2^63, which is inside this range.
        return format!("{number:.0}");
    }
    format!("{number}")
}

/// Renders a value the way `console.log` does.
pub fn inspect(value: &Value) -> String {
    inspect_with_depth(value, 0)
}

fn inspect_with_depth(value: &Value, depth: usize) -> String {
    if depth > 4 {
        return "…".to_string();
    }
    match value {
        Value::Str(text) if depth > 0 => format!("'{text}'"),
        Value::Array(items) => {
            let items = items.borrow();
            let rendered: Vec<String> = items
                .iter()
                .take(100)
                .map(|item| inspect_with_depth(item, depth + 1))
                .collect();
            let suffix = if items.len() > 100 { ", …" } else { "" };
            format!("[ {}{} ]", rendered.join(", "), suffix)
        }
        Value::Object(object) => {
            if object.own("message").is_some() {
                return value.to_js_string();
            }
            let entries = object.entries();
            if entries.is_empty() {
                return "{}".to_string();
            }
            let rendered: Vec<String> = entries
                .iter()
                .take(100)
                .map(|(key, item)| format!("{key}: {}", inspect_with_depth(item, depth + 1)))
                .collect();
            let class = object.class_name.borrow().clone();
            let prefix = if class == "Object" {
                String::new()
            } else {
                format!("{class} ")
            };
            format!("{prefix}{{ {} }}", rendered.join(", "))
        }
        other => other.to_js_string(),
    }
}

/// A lexical scope.
pub struct Scope {
    bindings: RefCell<Vec<(String, Binding)>>,
    parent: Option<Rc<Scope>>,
}

struct Binding {
    value: Value,
    mutable: bool,
}

impl Scope {
    pub fn root() -> Rc<Scope> {
        Rc::new(Scope {
            bindings: RefCell::new(Vec::new()),
            parent: None,
        })
    }

    pub fn child(parent: &Rc<Scope>) -> Rc<Scope> {
        Rc::new(Scope {
            bindings: RefCell::new(Vec::new()),
            parent: Some(parent.clone()),
        })
    }

    /// Declares a binding in this scope, replacing any of the same name.
    pub fn declare(&self, name: impl Into<String>, value: Value, mutable: bool) {
        let name = name.into();
        let mut bindings = self.bindings.borrow_mut();
        match bindings.iter_mut().find(|(existing, _)| *existing == name) {
            Some(slot) => slot.1 = Binding { value, mutable },
            None => bindings.push((name, Binding { value, mutable })),
        }
    }

    pub fn has_own(&self, name: &str) -> bool {
        self.bindings
            .borrow()
            .iter()
            .any(|(existing, _)| existing == name)
    }

    /// Looks a name up through the scope chain.
    pub fn lookup(&self, name: &str) -> Option<Value> {
        if let Some((_, binding)) = self
            .bindings
            .borrow()
            .iter()
            .find(|(existing, _)| existing == name)
        {
            return Some(binding.value.clone());
        }
        self.parent.as_ref()?.lookup(name)
    }

    /// Assigns to an existing binding.
    ///
    /// Returns `Err(true)` if the binding exists but is a constant, and
    /// `Err(false)` if there is no such binding.
    pub fn assign(&self, name: &str, value: Value) -> Result<(), bool> {
        {
            let mut bindings = self.bindings.borrow_mut();
            if let Some((_, binding)) = bindings.iter_mut().find(|(existing, _)| existing == name) {
                if !binding.mutable {
                    return Err(true);
                }
                binding.value = value;
                return Ok(());
            }
        }
        match &self.parent {
            Some(parent) => parent.assign(name, value),
            None => Err(false),
        }
    }

    /// The outermost scope, where `var` and undeclared assignments land.
    pub fn global(scope: &Rc<Scope>) -> Rc<Scope> {
        let mut current = scope.clone();
        while let Some(parent) = current.parent.clone() {
            current = parent;
        }
        current
    }

    pub fn names(&self) -> Vec<String> {
        self.bindings
            .borrow()
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting_matches_javascript() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(f64::NAN), "NaN");
        assert_eq!(format_number(f64::INFINITY), "Infinity");
        assert_eq!(format_number(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(format_number(1e21), "1e+21");
        assert_eq!(format_number(1e-7), "1e-7");
        assert_eq!(format_number(1234567.0), "1234567");
    }

    #[test]
    fn truthiness() {
        assert!(!Value::Undefined.truthy());
        assert!(!Value::Null.truthy());
        assert!(!Value::Number(0.0).truthy());
        assert!(!Value::Number(f64::NAN).truthy());
        assert!(!Value::string("").truthy());
        assert!(Value::string("0").truthy(), "a non-empty string is truthy");
        assert!(Value::Number(1.0).truthy());
        assert!(Value::array(vec![]).truthy(), "an empty array is truthy");
    }

    #[test]
    fn number_conversion() {
        assert_eq!(Value::string("42").to_number(), 42.0);
        assert_eq!(Value::string("  3.5 ").to_number(), 3.5);
        assert_eq!(Value::string("0x1f").to_number(), 31.0);
        assert_eq!(Value::string("").to_number(), 0.0);
        assert!(Value::string("abc").to_number().is_nan());
        assert_eq!(Value::Null.to_number(), 0.0);
        assert!(Value::Undefined.to_number().is_nan());
        assert_eq!(Value::Bool(true).to_number(), 1.0);
        assert_eq!(Value::array(vec![]).to_number(), 0.0);
        assert_eq!(Value::array(vec![Value::Number(7.0)]).to_number(), 7.0);
    }

    #[test]
    fn int32_conversion_wraps() {
        assert_eq!(Value::Number(5.9).to_int32(), 5);
        assert_eq!(Value::Number(-5.9).to_int32(), -5);
        assert_eq!(Value::Number(4294967296.0).to_int32(), 0);
        assert_eq!(Value::Number(f64::NAN).to_int32(), 0);
    }

    #[test]
    fn string_conversion() {
        assert_eq!(Value::Undefined.to_js_string(), "undefined");
        assert_eq!(Value::Null.to_js_string(), "null");
        assert_eq!(Value::Number(1.0).to_js_string(), "1");
        assert_eq!(
            Value::array(vec![Value::Number(1.0), Value::Null, Value::string("a")]).to_js_string(),
            "1,,a"
        );
        assert_eq!(
            Value::object(JsObject::new()).to_js_string(),
            "[object Object]"
        );
    }

    #[test]
    fn strict_equality_compares_identity_for_objects() {
        let a = Value::object(JsObject::new());
        let b = Value::object(JsObject::new());
        assert!(a.strict_equals(&a.clone()));
        assert!(!a.strict_equals(&b));
        assert!(Value::Number(1.0).strict_equals(&Value::Number(1.0)));
        assert!(!Value::Number(1.0).strict_equals(&Value::string("1")));
    }

    #[test]
    fn loose_equality_coerces() {
        assert!(Value::Number(1.0).loose_equals(&Value::string("1")));
        assert!(Value::Bool(true).loose_equals(&Value::Number(1.0)));
        assert!(Value::Null.loose_equals(&Value::Undefined));
        assert!(!Value::Null.loose_equals(&Value::Number(0.0)));
        assert!(!Value::Number(1.0).loose_equals(&Value::string("2")));
    }

    #[test]
    fn objects_keep_insertion_order() {
        let object = JsObject::new();
        object.set("b", Value::Number(1.0));
        object.set("a", Value::Number(2.0));
        object.set("b", Value::Number(3.0));
        assert_eq!(object.keys(), vec!["b", "a"]);
        assert_eq!(object.own("b").unwrap().to_number(), 3.0);
    }

    #[test]
    fn property_lookup_walks_the_prototype_chain() {
        let base = Rc::new(JsObject::new());
        base.set("shared", Value::Number(1.0));
        let derived = JsObject::new();
        derived.set_prototype(Some(base.clone()));
        derived.set("own", Value::Number(2.0));

        assert_eq!(derived.get("own").unwrap().to_number(), 2.0);
        assert_eq!(derived.get("shared").unwrap().to_number(), 1.0);
        assert!(derived.own("shared").is_none(), "not an own property");
        assert!(derived.get("missing").is_none());
    }

    #[test]
    fn a_prototype_cycle_does_not_hang() {
        let a = Rc::new(JsObject::new());
        let b = Rc::new(JsObject::new());
        a.set_prototype(Some(b.clone()));
        b.set_prototype(Some(a.clone()));
        assert!(a.get("nothing").is_none());
    }

    #[test]
    fn deleting_properties() {
        let object = JsObject::new();
        object.set("a", Value::Number(1.0));
        assert!(object.delete("a"));
        assert!(!object.delete("a"));
        assert!(object.is_empty());
    }

    #[test]
    fn scopes_shadow_and_chain() {
        let root = Scope::root();
        root.declare("a", Value::Number(1.0), true);
        let child = Scope::child(&root);
        child.declare("a", Value::Number(2.0), true);

        assert_eq!(child.lookup("a").unwrap().to_number(), 2.0);
        assert_eq!(root.lookup("a").unwrap().to_number(), 1.0);
        assert!(child.lookup("missing").is_none());
    }

    #[test]
    fn assignment_finds_the_declaring_scope() {
        let root = Scope::root();
        root.declare("a", Value::Number(1.0), true);
        let child = Scope::child(&root);
        assert!(child.assign("a", Value::Number(9.0)).is_ok());
        assert_eq!(root.lookup("a").unwrap().to_number(), 9.0);
    }

    #[test]
    fn constants_cannot_be_reassigned() {
        let scope = Scope::root();
        scope.declare("c", Value::Number(1.0), false);
        assert_eq!(scope.assign("c", Value::Number(2.0)), Err(true));
        assert_eq!(scope.assign("missing", Value::Number(2.0)), Err(false));
    }

    #[test]
    fn inspection_renders_nested_structures() {
        let object = JsObject::new();
        object.set("n", Value::Number(1.0));
        object.set("s", Value::string("x"));
        object.set("a", Value::array(vec![Value::Number(2.0)]));
        let rendered = inspect(&Value::object(object));
        assert!(rendered.contains("n: 1"), "{rendered}");
        assert!(rendered.contains("s: 'x'"), "{rendered}");
        assert!(rendered.contains("a: [ 2 ]"), "{rendered}");
    }

    #[test]
    fn top_level_strings_log_without_quotes() {
        assert_eq!(inspect(&Value::string("plain")), "plain");
    }

    #[test]
    fn type_of_reports_javascript_types() {
        assert_eq!(Value::Undefined.type_of(), "undefined");
        assert_eq!(Value::Null.type_of(), "object");
        assert_eq!(Value::Number(1.0).type_of(), "number");
        assert_eq!(Value::string("").type_of(), "string");
        assert_eq!(Value::Bool(true).type_of(), "boolean");
        assert_eq!(Value::array(vec![]).type_of(), "object");
    }
}
