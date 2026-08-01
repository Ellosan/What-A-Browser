//! The tree-walking interpreter.
//!
//! Two safety properties matter for a browser that runs untrusted code on the
//! same thread as its own interface: a script must not be able to hang the
//! browser, and it must not be able to overflow the stack. Both are enforced
//! here by a step budget and a call-depth limit, reported as
//! [`Control::Fatal`] — which `try`/`catch` deliberately cannot swallow.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::*;
use crate::builtins;
use crate::value::{
    format_number, Closure, HostObject, JsObject, NativeFunction, Scope, Slot, Value, WriteTarget,
};

/// How a statement or expression finished.
#[derive(Debug)]
pub enum Control {
    Return(Value),
    Break(Option<String>),
    Continue(Option<String>),
    /// A thrown value, catchable by `try`.
    Throw(Value),
    /// A resource limit was hit. Not catchable.
    Fatal(String),
}

impl Control {
    /// The message a host should report for this outcome.
    pub fn message(&self) -> String {
        match self {
            Control::Throw(value) => value.to_js_string(),
            Control::Fatal(message) => message.clone(),
            Control::Return(_) => "unexpected `return`".to_string(),
            Control::Break(_) => "unexpected `break`".to_string(),
            Control::Continue(_) => "unexpected `continue`".to_string(),
        }
    }

    pub fn is_fatal(&self) -> bool {
        matches!(self, Control::Fatal(_))
    }
}

/// A message captured from `console`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleMessage {
    pub level: ConsoleLevel,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

/// A callback queued by `setTimeout` or `setInterval`.
pub struct Timer {
    pub id: u32,
    pub callback: Value,
    pub args: Vec<Value>,
    /// Milliseconds requested by the script.
    pub delay: f64,
    pub repeating: bool,
}

/// Limits on one script run.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum number of statements and loop iterations.
    pub steps: u64,
    /// Maximum nested function calls.
    pub call_depth: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            // Enough for real page scripts, small enough that a runaway loop
            // stops in well under a second.
            steps: 5_000_000,
            call_depth: 200,
        }
    }
}

/// The interpreter, which owns the global scope and the script's side effects.
pub struct Interp {
    pub global: Rc<Scope>,
    limits: Limits,
    steps: u64,
    depth: u32,
    /// Everything the script logged.
    pub console: Vec<ConsoleMessage>,
    /// Timers the script queued, for the host to run later.
    pub timers: Vec<Timer>,
    next_timer_id: u32,
}

impl Default for Interp {
    fn default() -> Self {
        Interp::new()
    }
}

impl Interp {
    pub fn new() -> Self {
        Interp::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        let mut interp = Interp {
            global: Scope::root(),
            limits,
            steps: 0,
            depth: 0,
            console: Vec::new(),
            timers: Vec::new(),
            next_timer_id: 1,
        };
        builtins::install(&mut interp);
        interp
    }

    /// Declares a global binding, which is how a host installs `window` and
    /// `document`.
    pub fn define_global(&mut self, name: &str, value: Value) {
        self.global.declare(name, value, true);
    }

    pub fn global_value(&self, name: &str) -> Option<Value> {
        self.global.lookup(name)
    }

    /// Resets the step budget, so each script or event handler gets a fresh one.
    pub fn reset_budget(&mut self) {
        self.steps = 0;
    }

    pub fn steps_used(&self) -> u64 {
        self.steps
    }

    /// Parses and runs `source`, returning the value of its last expression.
    pub fn eval(&mut self, source: &str) -> Result<Value, String> {
        let program = crate::parser::parse(source).map_err(|error| error.to_string())?;
        self.run(&program).map_err(|control| control.message())
    }

    /// Runs an already-parsed program in the global scope.
    pub fn run(&mut self, program: &Program) -> Result<Value, Control> {
        let scope = self.global.clone();
        self.hoist(&program.body, &scope, true);
        let mut last = Value::Undefined;
        for statement in &program.body {
            match self.exec(statement, &scope)? {
                Value::Undefined => {}
                value => last = value,
            }
        }
        Ok(last)
    }

    /// Calls a callable value. Hosts use this to run event listeners.
    pub fn call(&mut self, callee: &Value, this: Value, args: &[Value]) -> Result<Value, Control> {
        match callee {
            Value::Function(closure) => self.call_closure(closure.clone(), this, args),
            Value::Native(native) => (native.func)(self, &this, args),
            other => Err(self.type_error(format!("{} is not a function", other.to_js_string()))),
        }
    }

    fn call_closure(
        &mut self,
        closure: Rc<Closure>,
        this: Value,
        args: &[Value],
    ) -> Result<Value, Control> {
        if self.depth >= self.limits.call_depth {
            return Err(Control::Fatal(format!(
                "maximum call depth of {} exceeded",
                self.limits.call_depth
            )));
        }
        self.charge(1)?;

        let scope = Scope::child(&closure.scope);
        // An arrow function inherits `this`; anything else binds its own.
        if !closure.function.is_arrow {
            let receiver = closure.this.clone().unwrap_or(this);
            scope.declare("this", receiver, false);
            scope.declare("arguments", Value::array(args.to_vec()), false);
            if let Some(home) = &closure.home_prototype {
                scope.declare("%home%", Value::Object(home.clone()), false);
            }
        }

        // Parameters, including defaults and destructuring.
        for (index, pattern) in closure.function.params.iter().enumerate() {
            let value = args.get(index).cloned().unwrap_or(Value::Undefined);
            self.bind_pattern(pattern, value, &scope, Some(true))?;
        }
        if let Some(rest) = &closure.function.rest {
            let extra: Vec<Value> = args
                .iter()
                .skip(closure.function.params.len())
                .cloned()
                .collect();
            scope.declare(rest.clone(), Value::array(extra), true);
        }

        self.depth += 1;
        let result = match &closure.function.body {
            FunctionBody::Expr(expr) => self.eval_expr(expr, &scope),
            FunctionBody::Block(body) => {
                self.hoist(body, &scope, true);
                let mut outcome = Ok(Value::Undefined);
                for statement in body.iter() {
                    match self.exec(statement, &scope) {
                        Ok(_) => {}
                        Err(Control::Return(value)) => {
                            outcome = Ok(value);
                            break;
                        }
                        Err(other) => {
                            outcome = Err(other);
                            break;
                        }
                    }
                }
                outcome
            }
        };
        self.depth -= 1;
        result
    }

    /// Spends part of the step budget.
    fn charge(&mut self, amount: u64) -> Result<(), Control> {
        self.steps += amount;
        if self.steps > self.limits.steps {
            return Err(Control::Fatal(
                "script took too long and was stopped".to_string(),
            ));
        }
        Ok(())
    }

    // ---- errors -----------------------------------------------------------

    /// Builds an error object of the given kind.
    pub fn make_error(&self, kind: &str, message: impl Into<String>) -> Value {
        let object = JsObject::with_class(kind);
        object.set("name", Value::string(kind));
        object.set("message", Value::string(message.into()));
        Value::object(object)
    }

    pub fn throw(&self, kind: &str, message: impl Into<String>) -> Control {
        Control::Throw(self.make_error(kind, message))
    }

    pub fn type_error(&self, message: impl Into<String>) -> Control {
        self.throw("TypeError", message)
    }

    pub fn range_error(&self, message: impl Into<String>) -> Control {
        self.throw("RangeError", message)
    }

    pub fn reference_error(&self, message: impl Into<String>) -> Control {
        self.throw("ReferenceError", message)
    }

    // ---- timers -----------------------------------------------------------

    /// Queues a timer, returning its id.
    pub fn queue_timer(
        &mut self,
        callback: Value,
        delay: f64,
        args: Vec<Value>,
        repeating: bool,
    ) -> u32 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            callback,
            args,
            delay,
            repeating,
        });
        id
    }

    pub fn cancel_timer(&mut self, id: u32) {
        self.timers.retain(|timer| timer.id != id);
    }

    /// Removes and returns the queued timers, in the order they were created.
    pub fn take_timers(&mut self) -> Vec<Timer> {
        std::mem::take(&mut self.timers)
    }

    pub fn log(&mut self, level: ConsoleLevel, text: String) {
        // A page cannot fill memory through the console.
        if self.console.len() < 1000 {
            self.console.push(ConsoleMessage { level, text });
        }
    }

    // ---- hoisting ---------------------------------------------------------

    /// Declares function declarations and `var` names before execution, so they
    /// can be referenced earlier in the body than they appear.
    fn hoist(&mut self, body: &[Stmt], scope: &Rc<Scope>, hoist_vars: bool) {
        for statement in body {
            if let Stmt::Function(function) = statement {
                if let Some(name) = &function.name {
                    let value = self.make_closure(function.clone(), scope, None);
                    scope.declare(name.clone(), value, true);
                }
            }
        }
        if hoist_vars {
            let mut names = Vec::new();
            collect_var_names(body, &mut names);
            for name in names {
                if !scope.has_own(&name) {
                    scope.declare(name, Value::Undefined, true);
                }
            }
        }
    }

    fn make_closure(
        &self,
        function: Rc<Function>,
        scope: &Rc<Scope>,
        home_prototype: Option<Rc<JsObject>>,
    ) -> Value {
        let prototype = Rc::new(JsObject::new());
        let name = function.name.clone().unwrap_or_default();
        Value::Function(Rc::new(Closure {
            function,
            scope: scope.clone(),
            this: None,
            prototype,
            home_prototype,
            name,
            properties: Rc::new(JsObject::new()),
        }))
    }

    // ---- statements -------------------------------------------------------

    fn exec_body(&mut self, body: &[Stmt], scope: &Rc<Scope>) -> Result<Value, Control> {
        self.hoist(body, scope, false);
        let mut last = Value::Undefined;
        for statement in body {
            last = self.exec(statement, scope)?;
        }
        Ok(last)
    }

    fn exec(&mut self, statement: &Stmt, scope: &Rc<Scope>) -> Result<Value, Control> {
        self.charge(1)?;
        match statement {
            Stmt::Empty => Ok(Value::Undefined),
            Stmt::Expr(expr) => self.eval_expr(expr, scope),
            Stmt::Block(body) => {
                let inner = Scope::child(scope);
                self.exec_body(body, &inner)
            }
            Stmt::VarDecl { kind, declarations } => {
                for (pattern, init) in declarations {
                    // A bare `var x;` has already been hoisted, and must not
                    // overwrite a value assigned earlier in the function.
                    if init.is_none() && *kind == DeclKind::Var {
                        continue;
                    }
                    let value = match init {
                        Some(expr) => self.eval_expr(expr, scope)?,
                        None => Value::Undefined,
                    };
                    // `var` assigns to the binding hoisting created in the
                    // enclosing function scope, so a declaration inside a block
                    // updates that one rather than shadowing it. `let` and
                    // `const` declare in the current scope.
                    let declare = match kind {
                        DeclKind::Var => None,
                        DeclKind::Let => Some(true),
                        DeclKind::Const => Some(false),
                    };
                    self.bind_pattern(pattern, value, scope, declare)?;
                }
                Ok(Value::Undefined)
            }
            Stmt::Function(function) => {
                // Hoisting already declared it; redeclare so a nested block's
                // function is visible in that block.
                if let Some(name) = &function.name {
                    if !scope.has_own(name) {
                        let value = self.make_closure(function.clone(), scope, None);
                        scope.declare(name.clone(), value, true);
                    }
                }
                Ok(Value::Undefined)
            }
            Stmt::Class(class) => {
                let value = self.eval_class(class, scope)?;
                if let Some(name) = &class.name {
                    scope.declare(name.clone(), value, true);
                }
                Ok(Value::Undefined)
            }
            Stmt::Return(argument) => {
                let value = match argument {
                    Some(expr) => self.eval_expr(expr, scope)?,
                    None => Value::Undefined,
                };
                Err(Control::Return(value))
            }
            Stmt::Throw(expr) => {
                let value = self.eval_expr(expr, scope)?;
                Err(Control::Throw(value))
            }
            Stmt::Break(label) => Err(Control::Break(label.clone())),
            Stmt::Continue(label) => Err(Control::Continue(label.clone())),
            Stmt::If {
                test,
                consequent,
                alternate,
            } => {
                if self.eval_expr(test, scope)?.truthy() {
                    self.exec(consequent, scope)
                } else if let Some(alternate) = alternate {
                    self.exec(alternate, scope)
                } else {
                    Ok(Value::Undefined)
                }
            }
            Stmt::While { test, body } => self.run_loop(None, scope, |interp, scope| {
                if !interp.eval_expr(test, scope)?.truthy() {
                    return Ok(false);
                }
                interp.exec(body, scope)?;
                Ok(true)
            }),
            Stmt::DoWhile { body, test } => {
                let mut first = true;
                self.run_loop(None, scope, |interp, scope| {
                    if !first && !interp.eval_expr(test, scope)?.truthy() {
                        return Ok(false);
                    }
                    first = false;
                    interp.exec(body, scope)?;
                    if !interp.eval_expr(test, scope)?.truthy() {
                        return Ok(false);
                    }
                    Ok(true)
                })
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.exec_for(
                None,
                init.as_deref(),
                test.as_ref(),
                update.as_ref(),
                body,
                scope,
            ),
            Stmt::ForOf { left, right, body } => {
                let iterable = self.eval_expr(right, scope)?;
                let items = self.iterate(&iterable)?;
                for item in items {
                    let inner = Scope::child(scope);
                    self.bind_for_target(left, item, &inner)?;
                    match self.exec(body, &inner) {
                        Ok(_) | Err(Control::Continue(None)) => {}
                        Err(Control::Break(None)) => break,
                        Err(other) => return Err(other),
                    }
                    self.charge(1)?;
                }
                Ok(Value::Undefined)
            }
            Stmt::ForIn { left, right, body } => {
                let object = self.eval_expr(right, scope)?;
                for key in self.enumerate_keys(&object) {
                    let inner = Scope::child(scope);
                    self.bind_for_target(left, Value::string(key), &inner)?;
                    match self.exec(body, &inner) {
                        Ok(_) | Err(Control::Continue(None)) => {}
                        Err(Control::Break(None)) => break,
                        Err(other) => return Err(other),
                    }
                    self.charge(1)?;
                }
                Ok(Value::Undefined)
            }
            Stmt::Labeled { label, body } => {
                let result = match &**body {
                    Stmt::While { .. }
                    | Stmt::DoWhile { .. }
                    | Stmt::For { .. }
                    | Stmt::ForIn { .. }
                    | Stmt::ForOf { .. } => self.exec_labeled_loop(label, body, scope),
                    other => self.exec(other, scope),
                };
                match result {
                    Err(Control::Break(Some(broken))) if broken == *label => Ok(Value::Undefined),
                    other => other,
                }
            }
            Stmt::Try {
                block,
                param,
                handler,
                finalizer,
            } => {
                let attempt = {
                    let inner = Scope::child(scope);
                    self.exec_body(block, &inner)
                };

                let after_catch = match attempt {
                    Err(Control::Throw(error)) if handler.is_some() => {
                        let inner = Scope::child(scope);
                        if let Some(pattern) = param {
                            self.bind_pattern(pattern, error, &inner, Some(true))?;
                        }
                        self.exec_body(handler.as_ref().expect("a handler"), &inner)
                    }
                    other => other,
                };

                // `finally` runs whatever happened, and its own control flow wins.
                if let Some(finalizer) = finalizer {
                    let inner = Scope::child(scope);
                    match self.exec_body(finalizer, &inner) {
                        Ok(_) => {}
                        Err(control) => return Err(control),
                    }
                }
                after_catch
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                let value = self.eval_expr(discriminant, scope)?;
                let inner = Scope::child(scope);
                // Find the matching case, or `default`.
                let mut start = None;
                for (index, case) in cases.iter().enumerate() {
                    if let Some(test) = &case.test {
                        let candidate = self.eval_expr(test, &inner)?;
                        if value.strict_equals(&candidate) {
                            start = Some(index);
                            break;
                        }
                    }
                }
                if start.is_none() {
                    start = cases.iter().position(|case| case.test.is_none());
                }
                let Some(start) = start else {
                    return Ok(Value::Undefined);
                };
                // Cases fall through until a `break`.
                for case in &cases[start..] {
                    match self.exec_body(&case.body, &inner) {
                        Ok(_) => {}
                        Err(Control::Break(None)) => return Ok(Value::Undefined),
                        Err(other) => return Err(other),
                    }
                }
                Ok(Value::Undefined)
            }
        }
    }

    /// Runs a loop body repeatedly, translating `break` and `continue`.
    fn run_loop(
        &mut self,
        label: Option<&str>,
        scope: &Rc<Scope>,
        mut step: impl FnMut(&mut Interp, &Rc<Scope>) -> Result<bool, Control>,
    ) -> Result<Value, Control> {
        loop {
            self.charge(1)?;
            let inner = Scope::child(scope);
            match step(self, &inner) {
                Ok(true) => continue,
                Ok(false) => return Ok(Value::Undefined),
                Err(Control::Break(None)) => return Ok(Value::Undefined),
                Err(Control::Break(Some(name))) if Some(name.as_str()) == label => {
                    return Ok(Value::Undefined)
                }
                Err(Control::Continue(None)) => continue,
                Err(Control::Continue(Some(name))) if Some(name.as_str()) == label => continue,
                Err(other) => return Err(other),
            }
        }
    }

    /// A `for` loop.
    ///
    /// `let` and `const` loop variables get a fresh binding each time round,
    /// which is what makes a closure created in the body capture the value it
    /// saw rather than the value the loop finished with.
    fn exec_for(
        &mut self,
        label: Option<&str>,
        init: Option<&Stmt>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
        scope: &Rc<Scope>,
    ) -> Result<Value, Control> {
        let outer = Scope::child(scope);
        let mut per_iteration: Vec<String> = Vec::new();
        let mut mutable = true;
        if let Some(init) = init {
            self.exec(init, &outer)?;
            if let Stmt::VarDecl { kind, .. } = init {
                if *kind != DeclKind::Var {
                    per_iteration = outer.names();
                    mutable = *kind != DeclKind::Const;
                }
            }
        }

        let mut env = if per_iteration.is_empty() {
            outer.clone()
        } else {
            copy_bindings(scope, &outer, &per_iteration, mutable)
        };
        loop {
            self.charge(1)?;
            if let Some(test) = test {
                if !self.eval_expr(test, &env)?.truthy() {
                    return Ok(Value::Undefined);
                }
            }
            let iteration = Scope::child(&env);
            match self.exec(body, &iteration) {
                Ok(_) | Err(Control::Continue(None)) => {}
                Err(Control::Continue(Some(name))) if Some(name.as_str()) == label => {}
                Err(Control::Break(None)) => return Ok(Value::Undefined),
                Err(Control::Break(Some(name))) if Some(name.as_str()) == label => {
                    return Ok(Value::Undefined)
                }
                Err(other) => return Err(other),
            }
            // The update runs against the *next* iteration's copy, so what the
            // body captured keeps the value it had.
            if !per_iteration.is_empty() {
                env = copy_bindings(scope, &env, &per_iteration, mutable);
            }
            if let Some(update) = update {
                self.eval_expr(update, &env)?;
            }
        }
    }

    /// A labelled loop, so `continue label` restarts the right loop.
    fn exec_labeled_loop(
        &mut self,
        label: &str,
        body: &Stmt,
        scope: &Rc<Scope>,
    ) -> Result<Value, Control> {
        match body {
            Stmt::While { test, body } => self.run_loop(Some(label), scope, |interp, scope| {
                if !interp.eval_expr(test, scope)?.truthy() {
                    return Ok(false);
                }
                interp.exec(body, scope)?;
                Ok(true)
            }),
            Stmt::For {
                init,
                test,
                update,
                body,
            } => self.exec_for(
                Some(label),
                init.as_deref(),
                test.as_ref(),
                update.as_ref(),
                body,
                scope,
            ),
            // The remaining loop forms handle their own labels well enough by
            // letting the label statement catch the break.
            other => self.exec(other, scope),
        }
    }

    fn bind_for_target(
        &mut self,
        target: &ForTarget,
        value: Value,
        scope: &Rc<Scope>,
    ) -> Result<(), Control> {
        match target {
            ForTarget::Decl(kind, pattern) => {
                self.bind_pattern(pattern, value, scope, Some(*kind != DeclKind::Const))
            }
            ForTarget::Pattern(pattern) => self.bind_pattern(pattern, value, scope, None),
        }
    }

    /// Binds `value` to `pattern`.
    ///
    /// `declare` is `Some(mutable)` to introduce new bindings, or `None` to
    /// assign to existing ones.
    fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        value: Value,
        scope: &Rc<Scope>,
        declare: Option<bool>,
    ) -> Result<(), Control> {
        match pattern {
            Pattern::Ident(name) => {
                match declare {
                    Some(mutable) => scope.declare(name.clone(), value, mutable),
                    None => match scope.assign(name, value.clone()) {
                        Ok(()) => {}
                        Err(true) => {
                            return Err(self
                                .type_error(format!("assignment to constant variable `{name}`")))
                        }
                        // An undeclared assignment creates a global, as
                        // non-strict JavaScript does.
                        Err(false) => Scope::global(scope).declare(name.clone(), value, true),
                    },
                }
                Ok(())
            }
            Pattern::Default(inner, default) => {
                let value = if value.is_nullish() {
                    self.eval_expr(default, scope)?
                } else {
                    value
                };
                self.bind_pattern(inner, value, scope, declare)
            }
            Pattern::Member(expr) => {
                let Expr::MemberAccess {
                    object, property, ..
                } = &**expr
                else {
                    return Err(self.type_error("invalid assignment target"));
                };
                let target = self.eval_expr(object, scope)?;
                let key = self.member_key(property, scope)?;
                self.set_member(&target, &key, value)
            }
            Pattern::Array { items, rest } => {
                let values = self.iterate(&value)?;
                for (index, item) in items.iter().enumerate() {
                    if let Some(item) = item {
                        let element = values.get(index).cloned().unwrap_or(Value::Undefined);
                        self.bind_pattern(item, element, scope, declare)?;
                    }
                }
                if let Some(rest) = rest {
                    let remainder: Vec<Value> = values.iter().skip(items.len()).cloned().collect();
                    self.bind_pattern(rest, Value::array(remainder), scope, declare)?;
                }
                Ok(())
            }
            Pattern::Object { props, rest } => {
                if value.is_nullish() {
                    return Err(self.type_error("cannot destructure a null or undefined value"));
                }
                let mut taken = Vec::new();
                for prop in props {
                    let key = match &prop.key {
                        PropKey::Computed(expr) => self.eval_expr(expr, scope)?.to_property_key(),
                        other => other.static_name().unwrap_or_default(),
                    };
                    let member = self.get_member(&value, &key)?;
                    taken.push(key);
                    self.bind_pattern(&prop.value, member, scope, declare)?;
                }
                if let Some(rest) = rest {
                    let object = JsObject::new();
                    for key in self.enumerate_keys(&value) {
                        if !taken.contains(&key) {
                            object.set(key.clone(), self.get_member(&value, &key)?);
                        }
                    }
                    let value = Value::object(object);
                    match declare {
                        Some(mutable) => scope.declare(rest.clone(), value, mutable),
                        None => {
                            let _ = scope.assign(rest, value);
                        }
                    }
                }
                Ok(())
            }
        }
    }

    // ---- expressions ------------------------------------------------------

    fn eval_expr(&mut self, expr: &Expr, scope: &Rc<Scope>) -> Result<Value, Control> {
        self.charge(1)?;
        match expr {
            Expr::Number(number) => Ok(Value::Number(*number)),
            Expr::Str(text) => Ok(Value::string(text.clone())),
            Expr::Bool(value) => Ok(Value::Bool(*value)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::This => Ok(scope.lookup("this").unwrap_or(Value::Undefined)),
            Expr::Super => Ok(scope.lookup("%home%").unwrap_or(Value::Undefined)),
            Expr::Ident(name) => match scope.lookup(name) {
                Some(value) => Ok(value),
                None => Err(self.reference_error(format!("{name} is not defined"))),
            },
            Expr::Template(elements) => {
                let mut out = String::new();
                for element in elements {
                    match element {
                        TemplateElem::Text(text) => out.push_str(text),
                        TemplateElem::Expr(expr) => {
                            out.push_str(&self.eval_expr(expr, scope)?.to_js_string())
                        }
                    }
                }
                Ok(Value::string(out))
            }
            Expr::Array(elements) => {
                let mut items = Vec::new();
                for element in elements {
                    match element {
                        ArrayElem::Hole => items.push(Value::Undefined),
                        ArrayElem::Item(expr) => items.push(self.eval_expr(expr, scope)?),
                        ArrayElem::Spread(expr) => {
                            let value = self.eval_expr(expr, scope)?;
                            items.extend(self.iterate(&value)?);
                        }
                    }
                }
                Ok(Value::array(items))
            }
            Expr::Object(props) => {
                let object = Rc::new(JsObject::new());
                for prop in props {
                    match prop {
                        ObjectProp::KeyValue { key, value } => {
                            let key = self.prop_key(key, scope)?;
                            let value = self.eval_expr(value, scope)?;
                            object.set(key, value);
                        }
                        ObjectProp::Method { key, function } => {
                            let key = self.prop_key(key, scope)?;
                            let value =
                                self.make_closure(function.clone(), scope, Some(object.clone()));
                            object.set(key, value);
                        }
                        ObjectProp::Getter { key, function } => {
                            let key = self.prop_key(key, scope)?;
                            let accessor =
                                self.make_closure(function.clone(), scope, Some(object.clone()));
                            object.define_getter(key, accessor);
                        }
                        ObjectProp::Setter { key, function } => {
                            let key = self.prop_key(key, scope)?;
                            let accessor =
                                self.make_closure(function.clone(), scope, Some(object.clone()));
                            object.define_setter(key, accessor);
                        }
                        ObjectProp::Spread(expr) => {
                            let value = self.eval_expr(expr, scope)?;
                            for key in self.enumerate_keys(&value) {
                                let item = self.get_member(&value, &key)?;
                                object.set(key, item);
                            }
                        }
                    }
                }
                Ok(Value::Object(object))
            }
            Expr::Function(function) => Ok(self.make_closure(function.clone(), scope, None)),
            Expr::Class(class) => self.eval_class(class, scope),
            Expr::Sequence(items) => {
                let mut last = Value::Undefined;
                for item in items {
                    last = self.eval_expr(item, scope)?;
                }
                Ok(last)
            }
            Expr::Unary { op, operand } => self.eval_unary(*op, operand, scope),
            Expr::Update { op, prefix, target } => {
                let old = self.eval_expr(target, scope)?.to_number();
                let new = match op {
                    UpdateOp::Increment => old + 1.0,
                    UpdateOp::Decrement => old - 1.0,
                };
                let pattern = (**target)
                    .clone()
                    .into_pattern()
                    .ok_or_else(|| self.type_error("invalid `++`/`--` target"))?;
                self.bind_pattern(&pattern, Value::Number(new), scope, None)?;
                Ok(Value::Number(if *prefix { new } else { old }))
            }
            Expr::Binary { op, left, right } => {
                let left = self.eval_expr(left, scope)?;
                let right = self.eval_expr(right, scope)?;
                self.binary(*op, left, right)
            }
            Expr::Logical { op, left, right } => {
                let left = self.eval_expr(left, scope)?;
                let take_right = match op {
                    LogicalOp::And => left.truthy(),
                    LogicalOp::Or => !left.truthy(),
                    LogicalOp::Nullish => left.is_nullish(),
                };
                if take_right {
                    self.eval_expr(right, scope)
                } else {
                    Ok(left)
                }
            }
            Expr::Assign { op, target, value } => {
                let value = match op {
                    None => self.eval_expr(value, scope)?,
                    Some(op) => {
                        let current = self.read_pattern(target, scope)?;
                        let operand = self.eval_expr(value, scope)?;
                        self.binary(*op, current, operand)?
                    }
                };
                self.bind_pattern(target, value.clone(), scope, None)?;
                Ok(value)
            }
            Expr::LogicalAssign { op, target, value } => {
                let current = self.read_pattern(target, scope)?;
                let should_assign = match op {
                    LogicalOp::And => current.truthy(),
                    LogicalOp::Or => !current.truthy(),
                    LogicalOp::Nullish => current.is_nullish(),
                };
                if !should_assign {
                    return Ok(current);
                }
                let value = self.eval_expr(value, scope)?;
                self.bind_pattern(target, value.clone(), scope, None)?;
                Ok(value)
            }
            Expr::Conditional {
                test,
                consequent,
                alternate,
            } => {
                if self.eval_expr(test, scope)?.truthy() {
                    self.eval_expr(consequent, scope)
                } else {
                    self.eval_expr(alternate, scope)
                }
            }
            Expr::MemberAccess {
                object,
                property,
                optional,
            } => {
                let target = self.eval_expr(object, scope)?;
                if *optional && target.is_nullish() {
                    return Ok(Value::Undefined);
                }
                if target.is_nullish() {
                    let key = self.member_key(property, scope)?;
                    return Err(self
                        .type_error(format!("cannot read `{key}` of {}", target.to_js_string())));
                }
                let key = self.member_key(property, scope)?;
                self.get_member(&target, &key)
            }
            Expr::Call {
                callee,
                args,
                optional,
            } => self.eval_call(callee, args, *optional, scope),
            Expr::New { callee, args } => {
                let constructor = self.eval_expr(callee, scope)?;
                let arguments = self.eval_arguments(args, scope)?;
                self.construct(&constructor, &arguments)
            }
        }
    }

    fn prop_key(&mut self, key: &PropKey, scope: &Rc<Scope>) -> Result<String, Control> {
        match key {
            PropKey::Computed(expr) => Ok(self.eval_expr(expr, scope)?.to_property_key()),
            other => Ok(other.static_name().unwrap_or_default()),
        }
    }

    fn member_key(&mut self, property: &Member, scope: &Rc<Scope>) -> Result<String, Control> {
        match property {
            Member::Ident(name) => Ok(name.clone()),
            Member::Computed(expr) => Ok(self.eval_expr(expr, scope)?.to_property_key()),
        }
    }

    /// Reads the current value of an assignment target.
    fn read_pattern(&mut self, pattern: &Pattern, scope: &Rc<Scope>) -> Result<Value, Control> {
        match pattern {
            Pattern::Ident(name) => Ok(scope.lookup(name).unwrap_or(Value::Undefined)),
            Pattern::Member(expr) => self.eval_expr(expr, scope),
            _ => Err(self.type_error("invalid compound assignment target")),
        }
    }

    fn eval_unary(
        &mut self,
        op: UnaryOp,
        operand: &Expr,
        scope: &Rc<Scope>,
    ) -> Result<Value, Control> {
        // `typeof missing` is not an error, unlike reading `missing`.
        if op == UnaryOp::TypeOf {
            if let Expr::Ident(name) = operand {
                if scope.lookup(name).is_none() {
                    return Ok(Value::string("undefined"));
                }
            }
        }
        if op == UnaryOp::Delete {
            if let Expr::MemberAccess {
                object, property, ..
            } = operand
            {
                let target = self.eval_expr(object, scope)?;
                let key = self.member_key(property, scope)?;
                return Ok(Value::Bool(match &target {
                    Value::Object(object) => object.delete(&key),
                    Value::Array(items) => {
                        if let Ok(index) = key.parse::<usize>() {
                            let mut items = items.borrow_mut();
                            if index < items.len() {
                                items[index] = Value::Undefined;
                                return Ok(Value::Bool(true));
                            }
                        }
                        false
                    }
                    _ => false,
                }));
            }
        }

        let value = self.eval_expr(operand, scope)?;
        Ok(match op {
            UnaryOp::Negate => Value::Number(-value.to_number()),
            UnaryOp::Plus => Value::Number(value.to_number()),
            UnaryOp::Not => Value::Bool(!value.truthy()),
            UnaryOp::BitNot => Value::Number(!value.to_int32() as f64),
            UnaryOp::TypeOf => Value::string(value.type_of()),
            UnaryOp::Void => Value::Undefined,
            UnaryOp::Delete => Value::Bool(true),
            // Without promises, `await` is the identity on a plain value.
            UnaryOp::Await => value,
        })
    }

    fn binary(&mut self, op: BinaryOp, left: Value, right: Value) -> Result<Value, Control> {
        use BinaryOp::*;
        let value = match op {
            Add => {
                // `+` concatenates when either side is a string, or an object
                // whose primitive form is one.
                fn concatenates(value: &Value) -> bool {
                    matches!(
                        value,
                        Value::Str(_) | Value::Object(_) | Value::Array(_) | Value::Host(_)
                    )
                }
                if concatenates(&left) || concatenates(&right) {
                    Value::string(format!("{}{}", left.to_js_string(), right.to_js_string()))
                } else {
                    Value::Number(left.to_number() + right.to_number())
                }
            }
            Subtract => Value::Number(left.to_number() - right.to_number()),
            Multiply => Value::Number(left.to_number() * right.to_number()),
            Divide => Value::Number(left.to_number() / right.to_number()),
            Remainder => Value::Number(left.to_number() % right.to_number()),
            Exponent => Value::Number(left.to_number().powf(right.to_number())),
            Equal => Value::Bool(left.loose_equals(&right)),
            NotEqual => Value::Bool(!left.loose_equals(&right)),
            StrictEqual => Value::Bool(left.strict_equals(&right)),
            StrictNotEqual => Value::Bool(!left.strict_equals(&right)),
            Less | LessEqual | Greater | GreaterEqual => {
                // Two strings compare lexicographically; anything else numerically.
                if let (Value::Str(a), Value::Str(b)) = (&left, &right) {
                    let ordering = a.as_str().cmp(b.as_str());
                    Value::Bool(match op {
                        Less => ordering.is_lt(),
                        LessEqual => ordering.is_le(),
                        Greater => ordering.is_gt(),
                        _ => ordering.is_ge(),
                    })
                } else {
                    let a = left.to_number();
                    let b = right.to_number();
                    if a.is_nan() || b.is_nan() {
                        Value::Bool(false)
                    } else {
                        Value::Bool(match op {
                            Less => a < b,
                            LessEqual => a <= b,
                            Greater => a > b,
                            _ => a >= b,
                        })
                    }
                }
            }
            ShiftLeft => Value::Number((left.to_int32() << (right.to_uint32() & 31)) as f64),
            ShiftRight => Value::Number((left.to_int32() >> (right.to_uint32() & 31)) as f64),
            ShiftRightUnsigned => {
                Value::Number((left.to_uint32() >> (right.to_uint32() & 31)) as f64)
            }
            BitAnd => Value::Number((left.to_int32() & right.to_int32()) as f64),
            BitOr => Value::Number((left.to_int32() | right.to_int32()) as f64),
            BitXor => Value::Number((left.to_int32() ^ right.to_int32()) as f64),
            In => {
                let key = left.to_property_key();
                Value::Bool(match &right {
                    Value::Object(object) => object.get(&key).is_some(),
                    Value::Array(items) => key
                        .parse::<usize>()
                        .map(|index| index < items.borrow().len())
                        .unwrap_or(false),
                    Value::Host(host) => host.get(&key).is_some(),
                    _ => false,
                })
            }
            InstanceOf => {
                // The built-in constructors are native functions with no
                // prototype object, so they are matched structurally instead.
                if let Value::Native(constructor) = &right {
                    return Ok(Value::Bool(builtins::native_instance_of(
                        constructor.name,
                        &left,
                    )));
                }
                let Value::Function(constructor) = &right else {
                    return Err(
                        self.type_error("the right-hand side of `instanceof` is not a constructor")
                    );
                };
                let mut current = match &left {
                    Value::Object(object) => object.prototype(),
                    _ => None,
                };
                let mut found = false;
                for _ in 0..64 {
                    let Some(prototype) = current else { break };
                    if Rc::ptr_eq(&prototype, &constructor.prototype) {
                        found = true;
                        break;
                    }
                    current = prototype.prototype();
                }
                Value::Bool(found)
            }
        };
        Ok(value)
    }

    fn eval_arguments(
        &mut self,
        args: &[Argument],
        scope: &Rc<Scope>,
    ) -> Result<Vec<Value>, Control> {
        let mut values = Vec::with_capacity(args.len());
        for argument in args {
            match argument {
                Argument::Normal(expr) => values.push(self.eval_expr(expr, scope)?),
                Argument::Spread(expr) => {
                    let value = self.eval_expr(expr, scope)?;
                    values.extend(self.iterate(&value)?);
                }
            }
        }
        Ok(values)
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Argument],
        optional: bool,
        scope: &Rc<Scope>,
    ) -> Result<Value, Control> {
        // `super(...)` calls the parent constructor with the current `this`.
        if matches!(callee, Expr::Super) {
            let parent = scope
                .lookup("%superctor%")
                .ok_or_else(|| self.type_error("`super` outside of a derived constructor"))?;
            let this = scope.lookup("this").unwrap_or(Value::Undefined);
            let arguments = self.eval_arguments(args, scope)?;
            return self.invoke_constructor_body(&parent, this, &arguments);
        }

        // A method call passes its object as the receiver.
        if let Expr::MemberAccess {
            object,
            property,
            optional: member_optional,
        } = callee
        {
            // `super.method(...)` looks the method up on the parent prototype.
            if matches!(**object, Expr::Super) {
                let key = self.member_key(property, scope)?;
                let home = scope.lookup("%home%");
                let this = scope.lookup("this").unwrap_or(Value::Undefined);
                let method = match home {
                    Some(Value::Object(prototype)) => {
                        prototype.prototype().and_then(|parent| parent.get(&key))
                    }
                    _ => None,
                };
                let Some(method) = method else {
                    return Err(self.type_error(format!("super.{key} is not a function")));
                };
                let arguments = self.eval_arguments(args, scope)?;
                return self.call(&method, this, &arguments);
            }

            let target = self.eval_expr(object, scope)?;
            if (*member_optional || optional) && target.is_nullish() {
                return Ok(Value::Undefined);
            }
            let key = self.member_key(property, scope)?;
            if target.is_nullish() {
                return Err(
                    self.type_error(format!("cannot call `{key}` of {}", target.to_js_string()))
                );
            }

            // Host objects handle their own methods, so a DOM call never needs a
            // function object to exist.
            if let Value::Host(host) = &target {
                if host.get(&key).is_none() {
                    let arguments = self.eval_arguments(args, scope)?;
                    return match host.invoke(&key, &arguments) {
                        Ok(value) => Ok(value),
                        Err(message) => Err(self.type_error(message)),
                    };
                }
            }

            let method = self.get_member(&target, &key)?;
            if optional && method.is_nullish() {
                return Ok(Value::Undefined);
            }
            if !method.is_callable() {
                return Err(self.type_error(format!("{key} is not a function")));
            }
            let arguments = self.eval_arguments(args, scope)?;
            return self.call(&method, target, &arguments);
        }

        let function = self.eval_expr(callee, scope)?;
        if optional && function.is_nullish() {
            return Ok(Value::Undefined);
        }
        let arguments = self.eval_arguments(args, scope)?;
        if !function.is_callable() {
            let name = match callee {
                Expr::Ident(name) => name.clone(),
                _ => function.to_js_string(),
            };
            return Err(self.type_error(format!("{name} is not a function")));
        }
        self.call(&function, Value::Undefined, &arguments)
    }

    /// `new F(...)`
    pub fn construct(&mut self, constructor: &Value, args: &[Value]) -> Result<Value, Control> {
        match constructor {
            Value::Function(closure) => {
                let instance = JsObject::with_class(if closure.name.is_empty() {
                    "Object".to_string()
                } else {
                    closure.name.clone()
                });
                instance.set_prototype(Some(closure.prototype.clone()));
                let this = Value::Object(Rc::new(instance));
                let result = self.invoke_constructor_body(constructor, this.clone(), args)?;
                // A constructor that returns an object returns that instead.
                Ok(match result {
                    Value::Object(_) => result,
                    _ => this,
                })
            }
            // Native constructors, such as `new Error(…)`, build their own value.
            Value::Native(native) => (native.func)(self, &Value::Undefined, args),
            other => Err(self.type_error(format!("{} is not a constructor", other.to_js_string()))),
        }
    }

    /// Runs a constructor body against an existing `this`.
    fn invoke_constructor_body(
        &mut self,
        constructor: &Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, Control> {
        match constructor {
            Value::Function(_) => self.call(constructor, this, args),
            Value::Native(native) => (native.func)(self, &this, args),
            Value::Undefined => Ok(Value::Undefined),
            other => Err(self.type_error(format!("{} is not a constructor", other.to_js_string()))),
        }
    }

    fn eval_class(&mut self, class: &Class, scope: &Rc<Scope>) -> Result<Value, Control> {
        let prototype = Rc::new(JsObject::new());
        let mut parent_constructor = Value::Undefined;

        if let Some(superclass) = &class.superclass {
            parent_constructor = self.eval_expr(superclass, scope)?;
            match &parent_constructor {
                Value::Function(parent) => {
                    prototype.set_prototype(Some(parent.prototype.clone()));
                }
                Value::Undefined | Value::Null => {}
                other => {
                    return Err(
                        self.type_error(format!("class cannot extend {}", other.to_js_string()))
                    )
                }
            }
        }

        // Methods and `super` resolve against a scope that knows the parent.
        let class_scope = Scope::child(scope);
        class_scope.declare("%superctor%", parent_constructor.clone(), false);

        // Instance fields are initialised at the top of the constructor.
        let mut field_setup: Vec<Stmt> = Vec::new();
        for member in &class.members {
            if member.kind != MemberKind::Field || member.is_static {
                continue;
            }
            let Some(name) = member.key.static_name() else {
                continue;
            };
            let value = member.value.clone().unwrap_or(Expr::Undefined);
            field_setup.push(Stmt::Expr(Expr::Assign {
                op: None,
                target: Box::new(Pattern::Member(Box::new(Expr::MemberAccess {
                    object: Box::new(Expr::This),
                    property: Member::Ident(name),
                    optional: false,
                }))),
                value: Box::new(value),
            }));
        }

        let declared_constructor = class
            .members
            .iter()
            .find(|member| member.kind == MemberKind::Constructor)
            .and_then(|member| member.function.clone());

        let constructor_function = match declared_constructor {
            Some(function) => {
                let body = match &function.body {
                    FunctionBody::Block(body) => {
                        let mut statements = Vec::with_capacity(body.len() + field_setup.len());
                        // Fields are set before the body runs. In a derived class
                        // the real order is after `super()`; this is close enough
                        // for field values that do not depend on the parent.
                        statements.extend(field_setup.clone());
                        statements.extend(body.iter().cloned());
                        FunctionBody::Block(Rc::new(statements))
                    }
                    other => other.clone(),
                };
                Rc::new(Function {
                    name: class.name.clone(),
                    params: function.params.clone(),
                    rest: function.rest.clone(),
                    body,
                    is_arrow: false,
                })
            }
            None => {
                // A default constructor forwards its arguments to the parent.
                let mut statements = Vec::new();
                if class.superclass.is_some() {
                    statements.push(Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::Super),
                        args: vec![Argument::Spread(Expr::Ident("arguments".to_string()))],
                        optional: false,
                    }));
                }
                statements.extend(field_setup);
                Rc::new(Function {
                    name: class.name.clone(),
                    params: Vec::new(),
                    rest: None,
                    body: FunctionBody::Block(Rc::new(statements)),
                    is_arrow: false,
                })
            }
        };

        let statics = Rc::new(JsObject::new());
        let closure = Rc::new(Closure {
            function: constructor_function,
            scope: class_scope.clone(),
            this: None,
            prototype: prototype.clone(),
            home_prototype: Some(prototype.clone()),
            name: class.name.clone().unwrap_or_default(),
            properties: statics.clone(),
        });
        let class_value = Value::Function(closure);

        // The class binds its own name so methods can refer to it.
        if let Some(name) = &class.name {
            class_scope.declare(name.clone(), class_value.clone(), false);
        }

        for member in &class.members {
            match member.kind {
                MemberKind::Constructor => {}
                MemberKind::Method => {
                    let Some(function) = &member.function else {
                        continue;
                    };
                    let key = self.prop_key(&member.key, &class_scope)?;
                    let method =
                        self.make_closure(function.clone(), &class_scope, Some(prototype.clone()));
                    if member.is_static {
                        statics.set(key, method);
                    } else {
                        prototype.set(key, method);
                    }
                }
                MemberKind::Getter | MemberKind::Setter => {
                    let Some(function) = &member.function else {
                        continue;
                    };
                    let key = self.prop_key(&member.key, &class_scope)?;
                    let accessor =
                        self.make_closure(function.clone(), &class_scope, Some(prototype.clone()));
                    let target = if member.is_static {
                        &statics
                    } else {
                        &prototype
                    };
                    if member.kind == MemberKind::Getter {
                        target.define_getter(key, accessor);
                    } else {
                        target.define_setter(key, accessor);
                    }
                }
                MemberKind::Field if member.is_static => {
                    let key = self.prop_key(&member.key, &class_scope)?;
                    let value = match &member.value {
                        Some(expr) => self.eval_expr(expr, &class_scope)?,
                        None => Value::Undefined,
                    };
                    statics.set(key, value);
                }
                MemberKind::Field => {}
            }
        }

        Ok(class_value)
    }

    // ---- property access --------------------------------------------------

    /// Reads a property from any value, including the built-in methods of
    /// strings, arrays, numbers and functions.
    pub fn get_member(&mut self, target: &Value, key: &str) -> Result<Value, Control> {
        match target {
            Value::Str(text) => {
                if key == "length" {
                    return Ok(Value::Number(text.chars().count() as f64));
                }
                if let Ok(index) = key.parse::<usize>() {
                    return Ok(text
                        .chars()
                        .nth(index)
                        .map(|ch| Value::string(ch.to_string()))
                        .unwrap_or(Value::Undefined));
                }
                Ok(builtins::string_member(key).unwrap_or(Value::Undefined))
            }
            Value::Array(items) => {
                if key == "length" {
                    return Ok(Value::Number(items.borrow().len() as f64));
                }
                if let Ok(index) = key.parse::<usize>() {
                    return Ok(items
                        .borrow()
                        .get(index)
                        .cloned()
                        .unwrap_or(Value::Undefined));
                }
                Ok(builtins::array_member(key).unwrap_or(Value::Undefined))
            }
            Value::Number(_) => Ok(builtins::number_member(key).unwrap_or(Value::Undefined)),
            Value::Bool(_) => Ok(builtins::boolean_member(key).unwrap_or(Value::Undefined)),
            Value::Object(object) => {
                let object = object.clone();
                if let Some(value) = self.read_slot(target, &object, key)? {
                    return Ok(value);
                }
                Ok(builtins::object_member(key).unwrap_or(Value::Undefined))
            }
            Value::Function(closure) => {
                match key {
                    "prototype" => return Ok(Value::Object(closure.prototype.clone())),
                    "name" => return Ok(Value::string(closure.name.clone())),
                    "length" => return Ok(Value::Number(closure.function.params.len() as f64)),
                    _ => {}
                }
                let properties = closure.properties.clone();
                if let Some(value) = self.read_slot(target, &properties, key)? {
                    return Ok(value);
                }
                Ok(builtins::function_member(key).unwrap_or(Value::Undefined))
            }
            Value::Native(native) => {
                if key == "name" && !native.properties.has_own("name") {
                    return Ok(Value::string(native.name));
                }
                let properties = native.properties.clone();
                if let Some(value) = self.read_slot(target, &properties, key)? {
                    return Ok(value);
                }
                Ok(builtins::function_member(key).unwrap_or(Value::Undefined))
            }
            Value::Host(host) => Ok(host.get(key).unwrap_or(Value::Undefined)),
            Value::Undefined | Value::Null => {
                Err(self.type_error(format!("cannot read `{key}` of {}", target.to_js_string())))
            }
        }
    }

    /// Reads a property from an object, calling a getter if that is what it
    /// finds. `receiver` is what the getter sees as `this`.
    fn read_slot(
        &mut self,
        receiver: &Value,
        object: &Rc<JsObject>,
        key: &str,
    ) -> Result<Option<Value>, Control> {
        match object.find(key) {
            Some(Slot::Value(value)) => Ok(Some(value)),
            Some(Slot::Getter(getter)) => Ok(Some(self.call(&getter, receiver.clone(), &[])?)),
            None => Ok(None),
        }
    }

    /// Writes a property to an object, routing through a setter if there is one.
    fn write_slot(
        &mut self,
        receiver: &Value,
        object: &Rc<JsObject>,
        key: &str,
        value: Value,
    ) -> Result<(), Control> {
        match object.write_target(key) {
            WriteTarget::Store => object.set(key, value),
            WriteTarget::Setter(setter) => {
                self.call(&setter, receiver.clone(), &[value])?;
            }
            // Assigning to a getter-only property is silently ignored outside
            // strict mode, which is what a page expects.
            WriteTarget::Ignore => {}
        }
        Ok(())
    }

    /// Writes a property.
    pub fn set_member(&mut self, target: &Value, key: &str, value: Value) -> Result<(), Control> {
        match target {
            Value::Object(object) => {
                let object = object.clone();
                self.write_slot(target, &object, key, value)
            }
            Value::Array(items) => {
                if key == "length" {
                    let length = value.to_number().max(0.0) as usize;
                    let mut items = items.borrow_mut();
                    items.resize(length, Value::Undefined);
                    return Ok(());
                }
                match key.parse::<usize>() {
                    Ok(index) => {
                        let mut items = items.borrow_mut();
                        if index >= items.len() {
                            items.resize(index + 1, Value::Undefined);
                        }
                        items[index] = value;
                        Ok(())
                    }
                    // A non-index property on an array is silently dropped.
                    Err(_) => Ok(()),
                }
            }
            Value::Function(closure) => {
                let properties = closure.properties.clone();
                self.write_slot(target, &properties, key, value)
            }
            Value::Native(native) => {
                let properties = native.properties.clone();
                self.write_slot(target, &properties, key, value)
            }
            Value::Host(host) => {
                if host.set(key, &value) {
                    Ok(())
                } else {
                    // Unknown DOM properties are ignored, as in a real browser.
                    Ok(())
                }
            }
            Value::Undefined | Value::Null => {
                Err(self.type_error(format!("cannot set `{key}` of {}", target.to_js_string())))
            }
            // Primitives silently discard writes.
            _ => Ok(()),
        }
    }

    /// The values of an iterable, for `for-of`, spreads and destructuring.
    pub fn iterate(&mut self, value: &Value) -> Result<Vec<Value>, Control> {
        match value {
            Value::Array(items) => Ok(items.borrow().clone()),
            Value::Str(text) => Ok(text
                .chars()
                .map(|ch| Value::string(ch.to_string()))
                .collect()),
            Value::Host(host) => {
                // A DOM collection exposes `length` and numeric indices.
                if let Some(length) = host.get("length") {
                    let count = length.to_number();
                    if count.is_finite() && count >= 0.0 {
                        let mut items = Vec::new();
                        for index in 0..count as usize {
                            items.push(host.get(&index.to_string()).unwrap_or(Value::Undefined));
                        }
                        return Ok(items);
                    }
                }
                Err(self.type_error(format!("{} is not iterable", host.type_name())))
            }
            Value::Object(object) => {
                // An array-like object is accepted, which covers most cases a
                // page relies on without a full iterator protocol.
                if let Some(length) = object.get("length") {
                    let count = length.to_number();
                    if count.is_finite() && count >= 0.0 {
                        let mut items = Vec::new();
                        for index in 0..count as usize {
                            items.push(object.get(&index.to_string()).unwrap_or(Value::Undefined));
                        }
                        return Ok(items);
                    }
                }
                Err(self.type_error("object is not iterable"))
            }
            other => Err(self.type_error(format!("{} is not iterable", other.to_js_string()))),
        }
    }

    /// The keys `for-in` and `Object.keys` see.
    pub fn enumerate_keys(&self, value: &Value) -> Vec<String> {
        match value {
            Value::Object(object) => object.keys(),
            Value::Array(items) => (0..items.borrow().len())
                .map(|index| index.to_string())
                .collect(),
            Value::Str(text) => (0..text.chars().count())
                .map(|index| index.to_string())
                .collect(),
            Value::Host(host) => host.own_keys(),
            Value::Function(closure) => closure.properties.keys(),
            Value::Native(native) => native.properties.keys(),
            _ => Vec::new(),
        }
    }
}

/// Collects the `var` names declared anywhere in `body` except inside nested
/// functions, which have their own scope.
fn collect_var_names(body: &[Stmt], names: &mut Vec<String>) {
    fn pattern_names(pattern: &Pattern, names: &mut Vec<String>) {
        match pattern {
            Pattern::Ident(name) => names.push(name.clone()),
            Pattern::Default(inner, _) => pattern_names(inner, names),
            Pattern::Array { items, rest } => {
                for item in items.iter().flatten() {
                    pattern_names(item, names);
                }
                if let Some(rest) = rest {
                    pattern_names(rest, names);
                }
            }
            Pattern::Object { props, rest } => {
                for prop in props {
                    pattern_names(&prop.value, names);
                }
                if let Some(rest) = rest {
                    names.push(rest.clone());
                }
            }
            Pattern::Member(_) => {}
        }
    }

    for statement in body {
        match statement {
            Stmt::VarDecl {
                kind: DeclKind::Var,
                declarations,
            } => {
                for (pattern, _) in declarations {
                    pattern_names(pattern, names);
                }
            }
            Stmt::Block(inner) => collect_var_names(inner, names),
            Stmt::If {
                consequent,
                alternate,
                ..
            } => {
                collect_var_names(std::slice::from_ref(consequent), names);
                if let Some(alternate) = alternate {
                    collect_var_names(std::slice::from_ref(alternate), names);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_var_names(std::slice::from_ref(init), names);
                }
                collect_var_names(std::slice::from_ref(body), names);
            }
            Stmt::ForIn { left, body, .. } | Stmt::ForOf { left, body, .. } => {
                if let ForTarget::Decl(DeclKind::Var, pattern) = left {
                    pattern_names(pattern, names);
                }
                collect_var_names(std::slice::from_ref(body), names);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_var_names(std::slice::from_ref(body), names)
            }
            Stmt::Try {
                block,
                handler,
                finalizer,
                ..
            } => {
                collect_var_names(block, names);
                if let Some(handler) = handler {
                    collect_var_names(handler, names);
                }
                if let Some(finalizer) = finalizer {
                    collect_var_names(finalizer, names);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_var_names(&case.body, names);
                }
            }
            _ => {}
        }
    }
}

/// A fresh scope holding copies of `names`, used for per-iteration loop
/// bindings.
fn copy_bindings(
    parent: &Rc<Scope>,
    from: &Rc<Scope>,
    names: &[String],
    mutable: bool,
) -> Rc<Scope> {
    let fresh = Scope::child(parent);
    for name in names {
        fresh.declare(
            name.clone(),
            from.lookup(name).unwrap_or(Value::Undefined),
            mutable,
        );
    }
    fresh
}

/// Builds a native function value.
pub fn native(
    name: &'static str,
    func: impl Fn(&mut Interp, &Value, &[Value]) -> Result<Value, Control> + 'static,
) -> Value {
    Value::Native(Rc::new(NativeFunction {
        name,
        properties: Rc::new(JsObject::new()),
        func: Box::new(func),
    }))
}

/// Convenience for hosts: wraps a [`HostObject`] as a value.
pub fn host_value<T: HostObject + 'static>(host: T) -> Value {
    Value::Host(Rc::new(host))
}

/// Formats a value for the console.
pub fn describe(value: &Value) -> String {
    crate::value::inspect(value)
}

/// Re-exported so hosts can build numbers without importing `value`.
pub fn number(value: f64) -> Value {
    Value::Number(value)
}

/// A shared cell, used by hosts that need mutable state behind a handle.
pub type Shared<T> = Rc<RefCell<T>>;

/// Wraps a value in a shared cell.
pub fn shared<T>(value: T) -> Shared<T> {
    Rc::new(RefCell::new(value))
}

/// Formats a number the way JavaScript does.
pub fn number_to_string(value: f64) -> String {
    format_number(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a script and renders its result as a string.
    fn run(source: &str) -> String {
        let mut interp = Interp::new();
        match interp.eval(source) {
            Ok(value) => value.to_js_string(),
            Err(message) => panic!("{source} failed: {message}"),
        }
    }

    /// Runs a script that is expected to fail, returning the message.
    fn fails(source: &str) -> String {
        let mut interp = Interp::new();
        match interp.eval(source) {
            Ok(value) => panic!("{source} unexpectedly produced {value:?}"),
            Err(message) => message,
        }
    }

    #[test]
    fn arithmetic_and_coercion() {
        assert_eq!(run("1 + 2 * 3"), "7");
        assert_eq!(run("(1 + 2) * 3"), "9");
        assert_eq!(run("7 % 3"), "1");
        assert_eq!(run("2 ** 10"), "1024");
        assert_eq!(run("7 / 2"), "3.5");
        assert_eq!(run("'a' + 1"), "a1");
        assert_eq!(run("1 + '2'"), "12");
        assert_eq!(run("'3' * '4'"), "12");
        assert_eq!(run("-'5'"), "-5");
        assert_eq!(run("+true"), "1");
        assert_eq!(run("1 / 0"), "Infinity");
        assert_eq!(run("0 / 0"), "NaN");
    }

    #[test]
    fn bitwise_and_shift_operators() {
        assert_eq!(run("5 & 3"), "1");
        assert_eq!(run("5 | 3"), "7");
        assert_eq!(run("5 ^ 3"), "6");
        assert_eq!(run("~5"), "-6");
        assert_eq!(run("1 << 4"), "16");
        assert_eq!(run("-16 >> 2"), "-4");
        assert_eq!(run("-16 >>> 28"), "15");
    }

    #[test]
    fn comparison_and_equality() {
        assert_eq!(run("1 < 2"), "true");
        assert_eq!(run("'a' < 'b'"), "true");
        assert_eq!(run("2 >= 2"), "true");
        assert_eq!(run("1 == '1'"), "true");
        assert_eq!(run("1 === '1'"), "false");
        assert_eq!(run("1 != '1'"), "false");
        assert_eq!(run("1 !== '1'"), "true");
        assert_eq!(run("NaN === NaN"), "false");
        assert_eq!(run("'a' in {a: 1}"), "true");
        assert_eq!(run("'b' in {a: 1}"), "false");
    }

    #[test]
    fn logical_operators_short_circuit() {
        assert_eq!(run("0 || 'fallback'"), "fallback");
        assert_eq!(run("'value' || 'fallback'"), "value");
        assert_eq!(run("1 && 2"), "2");
        assert_eq!(run("0 && 2"), "0");
        assert_eq!(run("null ?? 'default'"), "default");
        assert_eq!(
            run("0 ?? 'default'"),
            "0",
            "?? only catches null and undefined"
        );
        // The right-hand side must not be evaluated at all.
        assert_eq!(run("let n = 0; false && (n = 1); n"), "0");
        assert_eq!(run("let n = 0; true || (n = 1); n"), "0");
    }

    #[test]
    fn typeof_and_void_and_delete() {
        assert_eq!(run("typeof 1"), "number");
        assert_eq!(run("typeof 'a'"), "string");
        assert_eq!(run("typeof undefined"), "undefined");
        assert_eq!(run("typeof null"), "object");
        assert_eq!(run("typeof {}"), "object");
        assert_eq!(run("typeof (() => 1)"), "function");
        assert_eq!(
            run("typeof neverDeclared"),
            "undefined",
            "typeof does not throw on an unknown name"
        );
        assert_eq!(run("void 1"), "undefined");
        assert_eq!(run("const o = {a: 1}; delete o.a; 'a' in o"), "false");
    }

    #[test]
    fn declarations_and_scoping() {
        assert_eq!(run("let a = 1; { let a = 2; } a"), "1");
        assert_eq!(
            run("var a = 1; { var a = 2; } a"),
            "2",
            "var is function-scoped"
        );
        assert_eq!(run("const a = 1; a"), "1");
        assert!(fails("const a = 1; a = 2").contains("constant"));
        assert!(fails("neverDeclared").contains("neverDeclared"));
    }

    #[test]
    fn var_and_functions_are_hoisted() {
        assert_eq!(run("typeof later"), "undefined");
        assert_eq!(run("early(); function early() { return 1 } 'ok'"), "ok");
        assert_eq!(
            run("const before = hoisted(); function hoisted() { return 2 } before"),
            "2"
        );
        assert_eq!(
            run("function f() { const a = typeof v; var v = 1; return a } f()"),
            "undefined"
        );
    }

    #[test]
    fn conditionals() {
        assert_eq!(run("if (true) 1; else 2"), "1");
        assert_eq!(run("if (false) 1; else 2"), "2");
        assert_eq!(
            run("let a = 0; if (1 > 2) { a = 1 } else if (2 > 1) { a = 2 }; a"),
            "2"
        );
        assert_eq!(run("true ? 'y' : 'n'"), "y");
        assert_eq!(run("0 ? 'y' : 'n'"), "n");
    }

    #[test]
    fn loops() {
        assert_eq!(
            run("let n = 0; for (let i = 0; i < 5; i++) n += i; n"),
            "10"
        );
        assert_eq!(run("let n = 0; while (n < 3) n++; n"), "3");
        assert_eq!(run("let n = 0; do { n++ } while (n < 3); n"), "3");
        assert_eq!(
            run("let n = 0; do { n++ } while (false); n"),
            "1",
            "a do-while always runs once"
        );
        assert_eq!(
            run("let out = ''; for (const c of 'abc') out += c; out"),
            "abc"
        );
        assert_eq!(
            run("let out = ''; for (const k in {a: 1, b: 2}) out += k; out"),
            "ab"
        );
        assert_eq!(run("let n = 0; for (const v of [1, 2, 3]) n += v; n"), "6");
        // A `for` loop's binding is fresh each iteration, so closures capture
        // the value they saw.
        assert_eq!(
            run("const fns = []; for (let i = 0; i < 3; i++) fns.push(() => i); fns.map(f => f()).join('')"),
            "012"
        );
    }

    #[test]
    fn break_and_continue_including_labels() {
        assert_eq!(
            run("let n = 0; for (let i = 0; i < 10; i++) { if (i === 3) break; n++ } n"),
            "3"
        );
        assert_eq!(
            run("let n = 0; for (let i = 0; i < 5; i++) { if (i % 2) continue; n++ } n"),
            "3"
        );
        assert_eq!(
            run("let n = 0; outer: for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) continue outer; n++ } } n"),
            "3"
        );
        assert_eq!(
            run("let n = 0; outer: for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (i === 1) break outer; n++ } } n"),
            "3"
        );
    }

    #[test]
    fn switch_statements_fall_through() {
        assert_eq!(run("let r = ''; switch (2) { case 1: r = 'one'; break; case 2: r = 'two'; break; default: r = 'other' } r"), "two");
        assert_eq!(
            run("let r = ''; switch (9) { case 1: r = 'one'; break; default: r = 'other' } r"),
            "other"
        );
        assert_eq!(
            run("let r = ''; switch (1) { case 1: r += 'a'; case 2: r += 'b'; break; case 3: r += 'c' } r"),
            "ab",
            "a case without break falls through"
        );
        assert_eq!(
            run("let r = ''; switch ('1') { case 1: r = 'loose'; break; default: r = 'strict' } r"),
            "strict",
            "switch compares strictly"
        );
    }

    #[test]
    fn try_catch_finally() {
        assert_eq!(run("try { throw 'boom' } catch (e) { e }"), "boom");
        assert_eq!(run("try { 1 } finally { 2 }"), "1");
        assert_eq!(
            run("let r = ''; try { throw 1 } catch { r += 'c' } finally { r += 'f' } r"),
            "cf"
        );
        assert_eq!(
            run("function f() { try { return 'try' } finally { } } f()"),
            "try"
        );
        assert_eq!(
            run("let r = ''; function f() { try { return 1 } finally { r = 'ran' } } f(); r"),
            "ran",
            "finally runs even when the body returns"
        );
        assert_eq!(
            run("try { try { throw 1 } finally { } } catch (e) { 'outer' }"),
            "outer",
            "finally does not swallow the exception"
        );
        assert_eq!(run("try { null.x } catch (e) { e.name }"), "TypeError");
    }

    #[test]
    fn functions_and_closures() {
        assert_eq!(run("function add(a, b) { return a + b } add(1, 2)"), "3");
        assert_eq!(run("const add = (a, b) => a + b; add(1, 2)"), "3");
        assert_eq!(run("const one = () => 1; one()"), "1");
        assert_eq!(run("const id = x => x; id('a')"), "a");
        assert_eq!(run("(function () { return 'iife' })()"), "iife");
        assert_eq!(
            run("function counter() { let n = 0; return () => ++n } const c = counter(); c(); c()"),
            "2"
        );
        assert_eq!(
            run("function outer() { const a = 1; function inner() { return a } return inner() } outer()"),
            "1"
        );
        assert_eq!(run("function f(a = 2) { return a } f()"), "2");
        assert_eq!(run("function f(a = 2) { return a } f(5)"), "5");
        assert_eq!(
            run("function f(...rest) { return rest.length } f(1, 2, 3)"),
            "3"
        );
        assert_eq!(
            run("function f(a, ...rest) { return rest.join('') } f(1, 2, 3)"),
            "23"
        );
        assert_eq!(run("function f() { return arguments.length } f(1, 2)"), "2");
    }

    #[test]
    fn this_binding() {
        assert_eq!(
            run("const o = {n: 1, get() { return this.n }}; o.get()"),
            "1"
        );
        // An arrow function keeps the enclosing `this`.
        assert_eq!(
            run("const o = {n: 1, get() { const inner = () => this.n; return inner() }}; o.get()"),
            "1"
        );
        // A method pulled off its object loses its receiver.
        assert_eq!(
            run("const o = {n: 1, get() { return typeof this }}; const loose = o.get; loose()"),
            "undefined"
        );
    }

    #[test]
    fn objects_and_property_access() {
        assert_eq!(run("const o = {a: 1}; o.a"), "1");
        assert_eq!(run("const o = {a: 1}; o['a']"), "1");
        assert_eq!(run("const k = 'a'; ({a: 1})[k]"), "1");
        assert_eq!(run("const o = {}; o.a = 1; o.a"), "1");
        assert_eq!(run("const o = {a: {b: {c: 3}}}; o.a.b.c"), "3");
        assert_eq!(run("const a = 1; ({a}).a"), "1", "shorthand");
        assert_eq!(
            run("const k = 'x'; ({[k + '1']: 2})['x1']"),
            "2",
            "computed key"
        );
        assert_eq!(run("({...{a: 1}, b: 2}).a"), "1", "spread into a literal");
        assert_eq!(run("({a: 1, ...{a: 2}}).a"), "2", "later spread wins");
        assert_eq!(run("JSON.stringify({...[1, 2]})"), r#"{"0":1,"1":2}"#);
    }

    #[test]
    fn accessors_are_called_rather_than_returned() {
        assert_eq!(run("const o = {get double() { return 2 }}; o.double"), "2");
        assert_eq!(
            run("const o = {n: 1, get twice() { return this.n * 2 }}; o.n = 5; o.twice"),
            "10",
            "a getter sees the current receiver"
        );
        assert_eq!(
            run("const o = {_v: 0, set v(x) { this._v = x * 2 }}; o.v = 4; o._v"),
            "8"
        );
        assert_eq!(
            run("const o = {_v: 1, get v() { return this._v }, set v(x) { this._v = x }}; o.v = 7; o.v"),
            "7",
            "a get/set pair on the same key"
        );
        assert_eq!(
            run("const o = {get v() { return 1 }}; o.v = 9; o.v"),
            "1",
            "writing to a getter-only property is ignored"
        );
        assert_eq!(
            run("const o = {set v(x) {}}; String(o.v)"),
            "undefined",
            "a set-only property reads as undefined"
        );
        assert_eq!(
            run("Object.keys({a: 1, get b() { return 2 }}).length"),
            "2",
            "an accessor is an enumerable own property"
        );
        // A property merely named `get` is not an accessor.
        assert_eq!(run("({get: 1}).get"), "1");
        assert_eq!(run("({get() { return 2 }}).get()"), "2");
    }

    #[test]
    fn class_accessors() {
        assert_eq!(
            run("class C { constructor() { this.n = 3 } get squared() { return this.n * this.n } } new C().squared"),
            "9"
        );
        assert_eq!(
            run("class C { set value(v) { this.stored = v * 10 } } const c = new C(); c.value = 2; c.stored"),
            "20"
        );
        assert_eq!(
            run("class C { static get version() { return 2 } } C.version"),
            "2"
        );
        // An accessor lives on the prototype, so it is shared and not an own key.
        assert_eq!(
            run("class C { get a() { return 1 } } Object.keys(new C()).length"),
            "0"
        );
    }

    #[test]
    fn classes_and_inheritance() {
        assert_eq!(
            run("class A { constructor(n) { this.n = n } double() { return this.n * 2 } } new A(4).double()"),
            "8"
        );
        assert_eq!(
            run("class A { hi() { return 'a' } } class B extends A { } new B().hi()"),
            "a",
            "methods are inherited"
        );
        assert_eq!(
            run("class A { hi() { return 'a' } } class B extends A { hi() { return super.hi() + 'b' } } new B().hi()"),
            "ab"
        );
        assert_eq!(
            run("class A { constructor(n) { this.n = n } } class B extends A { constructor() { super(7) } } new B().n"),
            "7"
        );
        assert_eq!(
            run("class A { constructor(n) { this.n = n } } class B extends A { } new B(9).n"),
            "9",
            "a default constructor forwards its arguments"
        );
        assert_eq!(run("class A { field = 5 } new A().field"), "5");
        assert_eq!(run("class A { static count = 2 } A.count"), "2");
        assert_eq!(run("class A {} new A() instanceof A"), "true");
        assert_eq!(
            run("class A {} class B extends A {} new B() instanceof A"),
            "true"
        );
        assert_eq!(run("class A { m() {} } typeof A.prototype.m"), "function");
    }

    #[test]
    fn private_class_fields_are_reachable_but_not_enumerable() {
        assert_eq!(
            run("class C { #n = 1; bump() { return ++this.#n } } const c = new C(); c.bump()"),
            "2"
        );
        assert_eq!(
            run("class C { #n = 1; read() { return this.#n } } Object.keys(new C()).length"),
            "0",
            "a private field is hidden from enumeration"
        );
        assert_eq!(
            run("class C { #n = 1; read() { return this.#n } } JSON.stringify(new C())"),
            "{}"
        );
    }

    #[test]
    fn prototypes_are_shared_between_instances() {
        assert_eq!(
            run("function P() {} P.prototype.tag = 'shared'; new P().tag"),
            "shared"
        );
        assert_eq!(
            run("function P() { this.own = 1 } P.prototype.tag = 's'; const p = new P(); Object.keys(p).join('')"),
            "own",
            "an inherited property is not an own key"
        );
        assert_eq!(
            run("class A { m() { return 1 } } const a = new A(); a.m = () => 2; a.m()"),
            "2",
            "an own property shadows the prototype"
        );
    }

    #[test]
    fn constructors_returning_an_object_override_this() {
        assert_eq!(
            run("function F() { this.a = 1; return {a: 2} } new F().a"),
            "2"
        );
        assert_eq!(
            run("function F() { this.a = 1; return 5 } new F().a"),
            "1",
            "a primitive return is ignored"
        );
    }

    #[test]
    fn arrays() {
        assert_eq!(run("[1, 2, 3].length"), "3");
        assert_eq!(run("const a = [1]; a[2] = 3; a.length"), "3");
        assert_eq!(run("const a = [1]; a[2] = 3; String(a[1])"), "undefined");
        assert_eq!(run("const a = [1, 2, 3]; a.length = 1; a.toString()"), "1");
        assert_eq!(run("[...[1, 2], 3].toString()"), "1,2,3");
        assert_eq!(run("[...'ab'].toString()"), "a,b");
        assert_eq!(
            run("String([1, , 3][1])"),
            "undefined",
            "a hole reads as undefined"
        );
        assert_eq!(run("const a = []; a[0] = 1; a.length"), "1");
    }

    #[test]
    fn destructuring() {
        assert_eq!(run("const [a, b] = [1, 2]; a + b"), "3");
        assert_eq!(run("const [, b] = [1, 2]; b"), "2");
        assert_eq!(
            run("const [a, ...rest] = [1, 2, 3]; rest.toString()"),
            "2,3"
        );
        assert_eq!(run("const [a = 5] = []; a"), "5");
        assert_eq!(run("const {a, b} = {a: 1, b: 2}; a + b"), "3");
        assert_eq!(run("const {a: x} = {a: 1}; x"), "1");
        assert_eq!(run("const {a = 4} = {}; a"), "4");
        assert_eq!(
            run("const {a, ...rest} = {a: 1, b: 2}; JSON.stringify(rest)"),
            r#"{"b":2}"#
        );
        assert_eq!(run("const {a: {b}} = {a: {b: 3}}; b"), "3");
        assert_eq!(run("const [{a}] = [{a: 1}]; a"), "1");
        assert_eq!(
            run("function f({a, b = 2}) { return a + b } f({a: 1})"),
            "3"
        );
        assert_eq!(run("function f([a, b]) { return a + b } f([1, 2])"), "3");
        assert_eq!(run("let a, b; [a, b] = [1, 2]; a + b"), "3");
        assert_eq!(run("let a = 1, b = 2; [a, b] = [b, a]; `${a}${b}`"), "21");
        assert_eq!(run("const o = {}; ({a: o.x} = {a: 1}); o.x"), "1");
    }

    #[test]
    fn spread_in_calls() {
        assert_eq!(
            run("function f(a, b, c) { return a + b + c } f(...[1, 2, 3])"),
            "6"
        );
        assert_eq!(run("function f(a, b) { return a + b } f(1, ...[2])"), "3");
        assert_eq!(run("Math.max(...[1, 5, 3])"), "5");
    }

    #[test]
    fn template_literals() {
        assert_eq!(run("`plain`"), "plain");
        assert_eq!(run("const a = 2; `a is ${a}`"), "a is 2");
        assert_eq!(run("`${1 + 1} and ${'two'}`"), "2 and two");
        assert_eq!(run("const o = {n: 1}; `${o.n}`"), "1");
        assert_eq!(run("`nested ${`inner ${1}`}`"), "nested inner 1");
        assert_eq!(run("`line\nbreak`"), "line\nbreak");
    }

    #[test]
    fn assignment_operators() {
        assert_eq!(run("let a = 1; a += 2; a"), "3");
        assert_eq!(run("let a = 5; a -= 2; a"), "3");
        assert_eq!(run("let a = 2; a *= 3; a"), "6");
        assert_eq!(run("let a = 6; a /= 2; a"), "3");
        assert_eq!(run("let a = 7; a %= 4; a"), "3");
        assert_eq!(run("let a = 2; a **= 3; a"), "8");
        assert_eq!(run("let a = 1; a <<= 2; a"), "4");
        assert_eq!(run("let a = null; a ??= 'set'; a"), "set");
        assert_eq!(run("let a = 'kept'; a ??= 'set'; a"), "kept");
        assert_eq!(run("let a = 0; a ||= 'set'; a"), "set");
        assert_eq!(run("let a = 1; a &&= 'set'; a"), "set");
        assert_eq!(run("const o = {n: 1}; o.n += 2; o.n"), "3");
        assert_eq!(run("const a = [1]; a[0] += 2; a[0]"), "3");
    }

    #[test]
    fn increment_and_decrement() {
        assert_eq!(run("let a = 1; a++; a"), "2");
        assert_eq!(run("let a = 1; a--; a"), "0");
        assert_eq!(run("let a = 1; a++"), "1", "postfix returns the old value");
        assert_eq!(run("let a = 1; ++a"), "2", "prefix returns the new value");
        assert_eq!(run("const o = {n: 1}; o.n++; o.n"), "2");
        assert_eq!(run("const a = [1]; a[0]++; a[0]"), "2");
    }

    #[test]
    fn optional_chaining_and_calls() {
        assert_eq!(run("const o = null; String(o?.a)"), "undefined");
        assert_eq!(run("const o = {a: {b: 1}}; o?.a?.b"), "1");
        assert_eq!(run("const o = {}; String(o.a?.b)"), "undefined");
        assert_eq!(run("const o = {}; String(o.missing?.())"), "undefined");
        assert_eq!(run("const o = {f: () => 1}; o.f?.()"), "1");
        assert_eq!(run("const o = null; String(o?.['a'])"), "undefined");
        assert!(fails("const o = null; o.a").contains("cannot read"));
    }

    #[test]
    fn sequence_and_comma() {
        assert_eq!(run("let a = 0; (a = 1, a = 2); a"), "2");
        assert_eq!(run("let a = 1, b = 2; a + b"), "3");
    }

    #[test]
    fn a_thrown_error_propagates_out_of_nested_calls() {
        assert_eq!(
            run("function a() { b() } function b() { throw new Error('deep') } try { a() } catch (e) { e.message }"),
            "deep"
        );
        assert!(fails("throw new Error('uncaught')").contains("uncaught"));
    }

    #[test]
    fn recursion_within_the_limit_works() {
        assert_eq!(
            run("function fact(n) { return n <= 1 ? 1 : n * fact(n - 1) } fact(10)"),
            "3628800"
        );
        assert_eq!(
            run("function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2) } fib(15)"),
            "610"
        );
    }

    #[test]
    fn the_step_budget_stops_a_runaway_loop() {
        let mut interp = Interp::with_limits(Limits {
            steps: 1_000,
            call_depth: 32,
        });
        match interp.eval("while (true) {}") {
            Err(message) => assert!(message.contains("too long"), "{message}"),
            Ok(value) => panic!("expected the loop to be stopped, got {value:?}"),
        }
    }

    #[test]
    fn the_call_depth_limit_is_configurable() {
        let mut interp = Interp::with_limits(Limits {
            steps: 1_000_000,
            call_depth: 8,
        });
        let error = interp
            .eval("function f(n) { return n === 0 ? 0 : f(n - 1) } f(100)")
            .unwrap_err();
        assert!(error.contains("call depth"), "{error}");
    }

    #[test]
    fn a_fatal_limit_is_not_catchable() {
        let mut interp = Interp::with_limits(Limits {
            steps: 500,
            call_depth: 32,
        });
        let error = interp
            .eval("try { while (true) {} } catch (e) { 'caught' } finally { }")
            .unwrap_err();
        assert!(error.contains("too long"), "{error}");
    }

    #[test]
    fn a_host_object_receives_reads_writes_and_calls() {
        struct Probe {
            log: RefCell<Vec<String>>,
        }

        impl HostObject for Probe {
            fn type_name(&self) -> String {
                "Probe".to_string()
            }

            fn get(&self, key: &str) -> Option<Value> {
                self.log.borrow_mut().push(format!("get {key}"));
                match key {
                    "value" => Some(Value::Number(1.0)),
                    _ => None,
                }
            }

            fn set(&self, key: &str, value: &Value) -> bool {
                self.log
                    .borrow_mut()
                    .push(format!("set {key}={}", value.to_js_string()));
                true
            }

            fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
                self.log
                    .borrow_mut()
                    .push(format!("call {method}/{}", args.len()));
                match method {
                    "double" => Ok(Value::Number(args[0].to_number() * 2.0)),
                    other => Err(format!("no method {other}")),
                }
            }

            fn own_keys(&self) -> Vec<String> {
                vec!["value".to_string()]
            }

            fn identity(&self) -> usize {
                7
            }
        }

        let probe = Rc::new(Probe {
            log: RefCell::new(Vec::new()),
        });
        let mut interp = Interp::new();
        interp.define_global("probe", Value::Host(probe.clone()));

        assert_eq!(interp.eval("probe.value").unwrap().to_number(), 1.0);
        interp.eval("probe.other = 3").unwrap();
        assert_eq!(interp.eval("probe.double(4)").unwrap().to_number(), 8.0);
        assert_eq!(
            interp
                .eval("for (const k in probe) { } Object.keys(probe).length")
                .unwrap()
                .to_number(),
            1.0
        );

        let log = probe.log.borrow().clone();
        assert!(log.contains(&"get value".to_string()), "{log:?}");
        assert!(log.contains(&"set other=3".to_string()), "{log:?}");
        assert!(log.contains(&"call double/1".to_string()), "{log:?}");

        // A host object compares by identity, so two handles to the same thing
        // are `===`.
        interp.define_global("same", Value::Host(probe));
        assert!(interp.eval("probe === same").unwrap().truthy());
    }

    #[test]
    fn a_host_error_becomes_a_catchable_type_error() {
        assert_eq!(
            run("try { ({}).missing() } catch (e) { e.name }"),
            "TypeError"
        );
    }

    #[test]
    fn state_persists_between_evals() {
        let mut interp = Interp::new();
        interp.eval("let total = 0").unwrap();
        interp.eval("total += 5").unwrap();
        assert_eq!(interp.eval("total").unwrap().to_number(), 5.0);
    }

    #[test]
    fn empty_and_whitespace_programs_are_valid() {
        assert_eq!(run(""), "undefined");
        assert_eq!(run("   \n  "), "undefined");
        assert_eq!(run("// just a comment"), "undefined");
        assert_eq!(run("/* block */"), "undefined");
        assert_eq!(run(";;;"), "undefined");
    }

    #[test]
    fn automatic_semicolon_insertion() {
        assert_eq!(run("let a = 1\nlet b = 2\na + b"), "3");
        assert_eq!(run("function f() {\n  return 1\n}\nf()"), "1");
        assert_eq!(
            run("function f() {\n  return\n  1\n}\nString(f())"),
            "undefined",
            "a newline after return ends the statement"
        );
    }
}
