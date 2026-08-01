//! The syntax tree the parser produces and the interpreter walks.

use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Plus,
    Not,
    BitNot,
    TypeOf,
    Void,
    Delete,
    /// `await`, which without promises evaluates to its operand.
    Await,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Exponent,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    ShiftLeft,
    ShiftRight,
    ShiftRightUnsigned,
    BitAnd,
    BitOr,
    BitXor,
    In,
    InstanceOf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
    /// `??`
    Nullish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOp {
    Increment,
    Decrement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    Var,
    Let,
    Const,
}

/// A property name in an object literal or class body.
#[derive(Clone, Debug, PartialEq)]
pub enum PropKey {
    Ident(String),
    Str(String),
    Number(f64),
    Computed(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectProp {
    KeyValue {
        key: PropKey,
        value: Expr,
    },
    Method {
        key: PropKey,
        function: Rc<Function>,
    },
    /// `get name() { … }`
    Getter {
        key: PropKey,
        function: Rc<Function>,
    },
    /// `set name(value) { … }`
    Setter {
        key: PropKey,
        function: Rc<Function>,
    },
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayElem {
    /// A hole, as in `[1, , 3]`.
    Hole,
    Item(Expr),
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Argument {
    Normal(Expr),
    Spread(Expr),
}

/// How a member is named: `a.b` or `a[b]`.
#[derive(Clone, Debug, PartialEq)]
pub enum Member {
    Ident(String),
    Computed(Box<Expr>),
}

/// A binding target: a name, or a destructuring pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Ident(String),
    /// `{ a, b: c, ...rest }`
    Object {
        props: Vec<ObjectPatternProp>,
        rest: Option<String>,
    },
    /// `[a, , b, ...rest]`
    Array {
        items: Vec<Option<Pattern>>,
        rest: Option<Box<Pattern>>,
    },
    /// A pattern with a default value.
    Default(Box<Pattern>, Box<Expr>),
    /// A member expression used as an assignment target in destructuring.
    Member(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectPatternProp {
    /// The property to read.
    pub key: PropKey,
    /// Where to bind it.
    pub value: Pattern,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FunctionBody {
    Block(Rc<Vec<Stmt>>),
    /// An arrow function's concise body.
    Expr(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: Option<String>,
    pub params: Vec<Pattern>,
    /// The name bound to the rest of the arguments, if any.
    pub rest: Option<String>,
    pub body: FunctionBody,
    /// Arrow functions do not bind their own `this`.
    pub is_arrow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Constructor,
    Method,
    Field,
    /// `get name() { … }`
    Getter,
    /// `set name(value) { … }`
    Setter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassMember {
    pub key: PropKey,
    pub kind: MemberKind,
    pub is_static: bool,
    pub function: Option<Rc<Function>>,
    pub value: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: Option<String>,
    pub superclass: Option<Box<Expr>>,
    pub members: Vec<ClassMember>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TemplateElem {
    Text(String),
    Expr(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    This,
    Super,
    Ident(String),
    Template(Vec<TemplateElem>),
    Array(Vec<ArrayElem>),
    Object(Vec<ObjectProp>),
    Function(Rc<Function>),
    Class(Rc<Class>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Update {
        op: UpdateOp,
        prefix: bool,
        target: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        /// `None` for a plain `=`; `Some(op)` for `+=` and friends.
        op: Option<BinaryOp>,
        target: Box<Pattern>,
        value: Box<Expr>,
    },
    /// `a &&= b`, `a ||= b`, `a ??= b`.
    LogicalAssign {
        op: LogicalOp,
        target: Box<Pattern>,
        value: Box<Expr>,
    },
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
        /// `a?.()`
        optional: bool,
    },
    New {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    MemberAccess {
        object: Box<Expr>,
        property: Member,
        /// `a?.b`
        optional: bool,
    },
    Sequence(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    /// `None` for `default:`.
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
}

/// The left-hand side of a `for-in` or `for-of` loop.
#[derive(Clone, Debug, PartialEq)]
pub enum ForTarget {
    Decl(DeclKind, Pattern),
    Pattern(Pattern),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Expr(Expr),
    VarDecl {
        kind: DeclKind,
        declarations: Vec<(Pattern, Option<Expr>)>,
    },
    Function(Rc<Function>),
    Class(Rc<Class>),
    Return(Option<Expr>),
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    Block(Vec<Stmt>),
    For {
        init: Option<Box<Stmt>>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForIn {
        left: ForTarget,
        right: Expr,
        body: Box<Stmt>,
    },
    ForOf {
        left: ForTarget,
        right: Expr,
        body: Box<Stmt>,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    Break(Option<String>),
    Continue(Option<String>),
    Throw(Expr),
    Try {
        block: Vec<Stmt>,
        param: Option<Pattern>,
        handler: Option<Vec<Stmt>>,
        finalizer: Option<Vec<Stmt>>,
    },
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    Labeled {
        label: String,
        body: Box<Stmt>,
    },
    Empty,
}

/// A parsed program.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub body: Vec<Stmt>,
}

impl Expr {
    /// Can this expression be assigned to?
    pub fn is_assignable(&self) -> bool {
        matches!(self, Expr::Ident(_) | Expr::MemberAccess { .. })
    }

    /// Converts an already-parsed expression into an assignment target.
    ///
    /// The grammar cannot tell `{a} = b` from a block or an object literal until
    /// the `=` appears, so patterns are recovered from expressions after the
    /// fact.
    pub fn into_pattern(self) -> Option<Pattern> {
        match self {
            Expr::Ident(name) => Some(Pattern::Ident(name)),
            Expr::MemberAccess { .. } => Some(Pattern::Member(Box::new(self))),
            Expr::Assign {
                op: None,
                target,
                value,
            } => Some(Pattern::Default(target, value)),
            Expr::Array(elements) => {
                let mut items = Vec::new();
                let mut rest = None;
                for element in elements {
                    match element {
                        ArrayElem::Hole => items.push(None),
                        ArrayElem::Item(expr) => items.push(Some(expr.into_pattern()?)),
                        ArrayElem::Spread(expr) => {
                            rest = Some(Box::new(expr.into_pattern()?));
                        }
                    }
                }
                Some(Pattern::Array { items, rest })
            }
            Expr::Object(properties) => {
                let mut props = Vec::new();
                let mut rest = None;
                for property in properties {
                    match property {
                        ObjectProp::KeyValue { key, value } => props.push(ObjectPatternProp {
                            key,
                            value: value.into_pattern()?,
                        }),
                        ObjectProp::Spread(Expr::Ident(name)) => rest = Some(name),
                        _ => return None,
                    }
                }
                Some(Pattern::Object { props, rest })
            }
            _ => None,
        }
    }
}

impl PropKey {
    /// The static name of this key, if it has one.
    pub fn static_name(&self) -> Option<String> {
        match self {
            PropKey::Ident(name) | PropKey::Str(name) => Some(name.clone()),
            PropKey::Number(number) => Some(crate::value::format_number(*number)),
            PropKey::Computed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_members_are_assignable() {
        assert!(Expr::Ident("a".into()).is_assignable());
        assert!(Expr::MemberAccess {
            object: Box::new(Expr::Ident("a".into())),
            property: Member::Ident("b".into()),
            optional: false,
        }
        .is_assignable());
        assert!(!Expr::Number(1.0).is_assignable());
    }

    #[test]
    fn array_literals_become_array_patterns() {
        let expr = Expr::Array(vec![
            ArrayElem::Item(Expr::Ident("a".into())),
            ArrayElem::Hole,
            ArrayElem::Spread(Expr::Ident("rest".into())),
        ]);
        match expr.into_pattern().expect("a pattern") {
            Pattern::Array { items, rest } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], Some(Pattern::Ident("a".into())));
                assert_eq!(items[1], None);
                assert_eq!(rest, Some(Box::new(Pattern::Ident("rest".into()))));
            }
            other => panic!("expected an array pattern, got {other:?}"),
        }
    }

    #[test]
    fn object_literals_become_object_patterns() {
        let expr = Expr::Object(vec![
            ObjectProp::KeyValue {
                key: PropKey::Ident("a".into()),
                value: Expr::Ident("a".into()),
            },
            ObjectProp::Spread(Expr::Ident("rest".into())),
        ]);
        match expr.into_pattern().expect("a pattern") {
            Pattern::Object { props, rest } => {
                assert_eq!(props.len(), 1);
                assert_eq!(rest.as_deref(), Some("rest"));
            }
            other => panic!("expected an object pattern, got {other:?}"),
        }
    }

    #[test]
    fn literals_are_not_patterns() {
        assert!(Expr::Number(1.0).into_pattern().is_none());
        assert!(Expr::Array(vec![ArrayElem::Item(Expr::Number(1.0))])
            .into_pattern()
            .is_none());
    }

    #[test]
    fn keys_report_their_static_names() {
        assert_eq!(
            PropKey::Ident("a".into()).static_name().as_deref(),
            Some("a")
        );
        assert_eq!(PropKey::Number(3.0).static_name().as_deref(), Some("3"));
        assert!(PropKey::Computed(Expr::Ident("k".into()))
            .static_name()
            .is_none());
    }
}
