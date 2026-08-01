//! A recursive-descent parser with precedence climbing for expressions.
//!
//! Two pieces of JavaScript grammar need special handling and get it here:
//! automatic semicolon insertion, and telling an arrow function's parameter list
//! from a parenthesised expression — which is done by scanning ahead for `) =>`.

use std::rc::Rc;

use crate::ast::*;
use crate::lexer::{tokenize, SyntaxError, TemplatePiece, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Depth of nested function bodies, used to reject a stray `return`.
    function_depth: u32,
}

/// Parses a program.
pub fn parse(source: &str) -> Result<Program, SyntaxError> {
    let tokens = tokenize(source)?;
    Parser::new(tokens).parse_program()
}

/// Parses a single expression, used for template substitutions.
pub fn parse_expression(source: &str) -> Result<Expr, SyntaxError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    let expr = parser.expression()?;
    if !parser.peek().is_eof() {
        return Err(parser.error("unexpected trailing input"));
    }
    Ok(expr)
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            function_depth: 0,
        }
    }

    // ---- token helpers ----------------------------------------------------

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| self.tokens.last().expect("the token list ends with EOF"))
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens
            .get(self.pos + offset)
            .unwrap_or_else(|| self.tokens.last().expect("the token list ends with EOF"))
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn line(&self) -> u32 {
        self.peek().line
    }

    fn error(&self, message: impl Into<String>) -> SyntaxError {
        SyntaxError::new(message, self.line())
    }

    fn eat_punct(&mut self, text: &str) -> bool {
        if self.peek().is_punct(text) {
            self.advance();
            return true;
        }
        false
    }

    fn eat_keyword(&mut self, text: &str) -> bool {
        if self.peek().is_keyword(text) {
            self.advance();
            return true;
        }
        false
    }

    fn expect_punct(&mut self, text: &str) -> Result<(), SyntaxError> {
        if self.eat_punct(text) {
            return Ok(());
        }
        Err(self.error(format!("expected `{text}`")))
    }

    /// Consumes an identifier, accepting keywords in property position.
    fn expect_name(&mut self) -> Result<String, SyntaxError> {
        match self.peek().name() {
            Some(name) => {
                let name = name.to_string();
                self.advance();
                Ok(name)
            }
            None => Err(self.error("expected a name")),
        }
    }

    /// Consumes an identifier that is being *bound*, so keywords are rejected.
    fn expect_binding_name(&mut self) -> Result<String, SyntaxError> {
        match &self.peek().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            // These are only contextually reserved, so they may be bound.
            TokenKind::Keyword(keyword @ ("of" | "static" | "let" | "undefined" | "await")) => {
                let name = keyword.to_string();
                self.advance();
                Ok(name)
            }
            _ => Err(self.error("expected a variable name")),
        }
    }

    /// Automatic semicolon insertion: a statement may end at `;`, at `}`, at end
    /// of input, or wherever a line break makes the next token a new statement.
    fn consume_semicolon(&mut self) -> Result<(), SyntaxError> {
        if self.eat_punct(";") {
            return Ok(());
        }
        let token = self.peek();
        if token.is_eof() || token.is_punct("}") || token.newline_before {
            return Ok(());
        }
        Err(self.error("expected `;`"))
    }

    /// Is `async` being used as a modifier here rather than as a name?
    fn at_async_function(&self) -> bool {
        let is_async = matches!(&self.peek().kind, TokenKind::Ident(name) if name == "async");
        if !is_async {
            return false;
        }
        let next = self.peek_at(1);
        next.is_keyword("function")
            || next.is_punct("(")
            // `async x => …`
            || (matches!(next.kind, TokenKind::Ident(_)) && self.peek_at(2).is_punct("=>"))
    }

    // ---- program and statements -------------------------------------------

    pub fn parse_program(&mut self) -> Result<Program, SyntaxError> {
        let mut body = Vec::new();
        while !self.peek().is_eof() {
            body.push(self.statement()?);
        }
        Ok(Program { body })
    }

    fn statement(&mut self) -> Result<Stmt, SyntaxError> {
        if self.eat_punct(";") {
            return Ok(Stmt::Empty);
        }
        if self.peek().is_punct("{") {
            return Ok(Stmt::Block(self.block()?));
        }

        // A label is an identifier followed by a colon.
        if matches!(self.peek().kind, TokenKind::Ident(_)) && self.peek_at(1).is_punct(":") {
            let label = self.expect_binding_name()?;
            self.expect_punct(":")?;
            let body = Box::new(self.statement()?);
            return Ok(Stmt::Labeled { label, body });
        }

        match &self.peek().kind {
            TokenKind::Keyword("var") => self.var_declaration(DeclKind::Var),
            TokenKind::Keyword("let") => {
                // `let` is only a declaration when a binding follows.
                let next = self.peek_at(1);
                if matches!(next.kind, TokenKind::Ident(_))
                    || next.is_punct("[")
                    || next.is_punct("{")
                {
                    self.var_declaration(DeclKind::Let)
                } else {
                    self.expression_statement()
                }
            }
            TokenKind::Keyword("const") => self.var_declaration(DeclKind::Const),
            TokenKind::Keyword("function") => {
                let function = self.function(true)?;
                Ok(Stmt::Function(function))
            }
            TokenKind::Keyword("class") => {
                let class = self.class()?;
                Ok(Stmt::Class(class))
            }
            TokenKind::Keyword("if") => self.if_statement(),
            TokenKind::Keyword("for") => self.for_statement(),
            TokenKind::Keyword("while") => {
                self.advance();
                self.expect_punct("(")?;
                let test = self.expression()?;
                self.expect_punct(")")?;
                let body = Box::new(self.statement()?);
                Ok(Stmt::While { test, body })
            }
            TokenKind::Keyword("do") => {
                self.advance();
                let body = Box::new(self.statement()?);
                if !self.eat_keyword("while") {
                    return Err(self.error("expected `while`"));
                }
                self.expect_punct("(")?;
                let test = self.expression()?;
                self.expect_punct(")")?;
                let _ = self.eat_punct(";");
                Ok(Stmt::DoWhile { body, test })
            }
            TokenKind::Keyword("return") => {
                if self.function_depth == 0 {
                    return Err(self.error("`return` outside of a function"));
                }
                self.advance();
                // `return` with a line break after it returns undefined.
                let argument = if self.peek().is_punct(";")
                    || self.peek().is_punct("}")
                    || self.peek().is_eof()
                    || self.peek().newline_before
                {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.consume_semicolon()?;
                Ok(Stmt::Return(argument))
            }
            TokenKind::Keyword("break") | TokenKind::Keyword("continue") => {
                let is_break = self.peek().is_keyword("break");
                self.advance();
                let label = if matches!(self.peek().kind, TokenKind::Ident(_))
                    && !self.peek().newline_before
                {
                    Some(self.expect_binding_name()?)
                } else {
                    None
                };
                self.consume_semicolon()?;
                Ok(if is_break {
                    Stmt::Break(label)
                } else {
                    Stmt::Continue(label)
                })
            }
            TokenKind::Keyword("throw") => {
                self.advance();
                let argument = self.expression()?;
                self.consume_semicolon()?;
                Ok(Stmt::Throw(argument))
            }
            TokenKind::Keyword("try") => self.try_statement(),
            TokenKind::Keyword("switch") => self.switch_statement(),
            TokenKind::Keyword("debugger") => {
                self.advance();
                self.consume_semicolon()?;
                Ok(Stmt::Empty)
            }
            _ if self.at_async_function() && self.peek_at(1).is_keyword("function") => {
                self.advance();
                let function = self.function(true)?;
                Ok(Stmt::Function(function))
            }
            _ => self.expression_statement(),
        }
    }

    fn expression_statement(&mut self) -> Result<Stmt, SyntaxError> {
        let expr = self.expression()?;
        self.consume_semicolon()?;
        Ok(Stmt::Expr(expr))
    }

    fn block(&mut self) -> Result<Vec<Stmt>, SyntaxError> {
        self.expect_punct("{")?;
        let mut body = Vec::new();
        while !self.peek().is_punct("}") {
            if self.peek().is_eof() {
                return Err(self.error("unterminated block"));
            }
            body.push(self.statement()?);
        }
        self.expect_punct("}")?;
        Ok(body)
    }

    fn var_declaration(&mut self, kind: DeclKind) -> Result<Stmt, SyntaxError> {
        self.advance(); // var / let / const
        let declarations = self.declarator_list()?;
        self.consume_semicolon()?;
        Ok(Stmt::VarDecl { kind, declarations })
    }

    fn declarator_list(&mut self) -> Result<Vec<(Pattern, Option<Expr>)>, SyntaxError> {
        let mut declarations = Vec::new();
        loop {
            let pattern = self.binding_pattern()?;
            let init = if self.eat_punct("=") {
                Some(self.assignment_expression()?)
            } else {
                None
            };
            declarations.push((pattern, init));
            if !self.eat_punct(",") {
                break;
            }
        }
        Ok(declarations)
    }

    /// A binding target: a name, an array pattern or an object pattern.
    fn binding_pattern(&mut self) -> Result<Pattern, SyntaxError> {
        if self.peek().is_punct("[") {
            self.advance();
            let mut items = Vec::new();
            let mut rest = None;
            while !self.peek().is_punct("]") {
                if self.eat_punct(",") {
                    items.push(None);
                    continue;
                }
                if self.eat_punct("...") {
                    rest = Some(Box::new(self.binding_pattern()?));
                } else {
                    let mut pattern = self.binding_pattern()?;
                    if self.eat_punct("=") {
                        let default = self.assignment_expression()?;
                        pattern = Pattern::Default(Box::new(pattern), Box::new(default));
                    }
                    items.push(Some(pattern));
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("]")?;
            return Ok(Pattern::Array { items, rest });
        }

        if self.peek().is_punct("{") {
            self.advance();
            let mut props = Vec::new();
            let mut rest = None;
            while !self.peek().is_punct("}") {
                if self.eat_punct("...") {
                    rest = Some(self.expect_binding_name()?);
                } else {
                    let key = self.property_key()?;
                    let mut value = if self.eat_punct(":") {
                        self.binding_pattern()?
                    } else {
                        let name = key
                            .static_name()
                            .ok_or_else(|| self.error("a computed key needs a binding"))?;
                        Pattern::Ident(name)
                    };
                    if self.eat_punct("=") {
                        let default = self.assignment_expression()?;
                        value = Pattern::Default(Box::new(value), Box::new(default));
                    }
                    props.push(ObjectPatternProp { key, value });
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            return Ok(Pattern::Object { props, rest });
        }

        Ok(Pattern::Ident(self.expect_binding_name()?))
    }

    fn if_statement(&mut self) -> Result<Stmt, SyntaxError> {
        self.advance();
        self.expect_punct("(")?;
        let test = self.expression()?;
        self.expect_punct(")")?;
        let consequent = Box::new(self.statement()?);
        let alternate = if self.eat_keyword("else") {
            Some(Box::new(self.statement()?))
        } else {
            None
        };
        Ok(Stmt::If {
            test,
            consequent,
            alternate,
        })
    }

    fn for_statement(&mut self) -> Result<Stmt, SyntaxError> {
        self.advance();
        self.expect_punct("(")?;

        // An empty initialiser: `for (;;)`.
        if self.peek().is_punct(";") {
            return self.classic_for(None);
        }

        let declaration_kind = match &self.peek().kind {
            TokenKind::Keyword("var") => Some(DeclKind::Var),
            TokenKind::Keyword("let") => Some(DeclKind::Let),
            TokenKind::Keyword("const") => Some(DeclKind::Const),
            _ => None,
        };

        if let Some(kind) = declaration_kind {
            self.advance();
            let pattern = self.binding_pattern()?;
            if self.eat_keyword("of") {
                return self.for_of_or_in(ForTarget::Decl(kind, pattern), true);
            }
            if self.eat_keyword("in") {
                return self.for_of_or_in(ForTarget::Decl(kind, pattern), false);
            }
            // A plain `for` loop, whose initialiser may declare several names.
            let init = if self.eat_punct("=") {
                let value = self.assignment_expression()?;
                let mut declarations = vec![(pattern, Some(value))];
                if self.eat_punct(",") {
                    declarations.extend(self.declarator_list()?);
                }
                Stmt::VarDecl { kind, declarations }
            } else {
                let mut declarations = vec![(pattern, None)];
                if self.eat_punct(",") {
                    declarations.extend(self.declarator_list()?);
                }
                Stmt::VarDecl { kind, declarations }
            };
            return self.classic_for(Some(Box::new(init)));
        }

        // No declaration: an expression, possibly a for-in/of target.
        let expr = self.expression()?;
        if self.eat_keyword("of") {
            let pattern = expr
                .into_pattern()
                .ok_or_else(|| self.error("invalid `for…of` target"))?;
            return self.for_of_or_in(ForTarget::Pattern(pattern), true);
        }
        if self.eat_keyword("in") {
            let pattern = expr
                .into_pattern()
                .ok_or_else(|| self.error("invalid `for…in` target"))?;
            return self.for_of_or_in(ForTarget::Pattern(pattern), false);
        }
        self.classic_for(Some(Box::new(Stmt::Expr(expr))))
    }

    fn classic_for(&mut self, init: Option<Box<Stmt>>) -> Result<Stmt, SyntaxError> {
        self.expect_punct(";")?;
        let test = if self.peek().is_punct(";") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect_punct(";")?;
        let update = if self.peek().is_punct(")") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect_punct(")")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::For {
            init,
            test,
            update,
            body,
        })
    }

    fn for_of_or_in(&mut self, left: ForTarget, is_of: bool) -> Result<Stmt, SyntaxError> {
        let right = self.expression()?;
        self.expect_punct(")")?;
        let body = Box::new(self.statement()?);
        Ok(if is_of {
            Stmt::ForOf { left, right, body }
        } else {
            Stmt::ForIn { left, right, body }
        })
    }

    fn try_statement(&mut self) -> Result<Stmt, SyntaxError> {
        self.advance();
        let block = self.block()?;
        let mut param = None;
        let mut handler = None;
        if self.eat_keyword("catch") {
            if self.eat_punct("(") {
                param = Some(self.binding_pattern()?);
                self.expect_punct(")")?;
            }
            handler = Some(self.block()?);
        }
        let finalizer = if self.eat_keyword("finally") {
            Some(self.block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.error("`try` needs a `catch` or a `finally`"));
        }
        Ok(Stmt::Try {
            block,
            param,
            handler,
            finalizer,
        })
    }

    fn switch_statement(&mut self) -> Result<Stmt, SyntaxError> {
        self.advance();
        self.expect_punct("(")?;
        let discriminant = self.expression()?;
        self.expect_punct(")")?;
        self.expect_punct("{")?;

        let mut cases = Vec::new();
        while !self.peek().is_punct("}") {
            if self.peek().is_eof() {
                return Err(self.error("unterminated `switch`"));
            }
            let test = if self.eat_keyword("case") {
                let test = self.expression()?;
                Some(test)
            } else if self.eat_keyword("default") {
                None
            } else {
                return Err(self.error("expected `case` or `default`"));
            };
            self.expect_punct(":")?;

            let mut body = Vec::new();
            while !self.peek().is_punct("}")
                && !self.peek().is_keyword("case")
                && !self.peek().is_keyword("default")
            {
                if self.peek().is_eof() {
                    return Err(self.error("unterminated `switch`"));
                }
                body.push(self.statement()?);
            }
            cases.push(SwitchCase { test, body });
        }
        self.expect_punct("}")?;
        Ok(Stmt::Switch {
            discriminant,
            cases,
        })
    }

    // ---- functions and classes -------------------------------------------

    /// Parses `function name(params) { body }`. The `function` keyword must be
    /// the current token.
    fn function(&mut self, named: bool) -> Result<Rc<Function>, SyntaxError> {
        self.advance(); // `function`
                        // Generators are parsed but the `*` is ignored; `yield` is not supported.
        let _ = self.eat_punct("*");
        // A declaration must be named; an expression may be.
        let name = if self.peek().name().is_some() {
            Some(self.expect_binding_name()?)
        } else if named {
            return Err(self.error("a function declaration needs a name"));
        } else {
            None
        };
        let (params, rest) = self.parameter_list()?;
        self.function_depth += 1;
        let body = self.block();
        self.function_depth -= 1;
        Ok(Rc::new(Function {
            name,
            params,
            rest,
            body: FunctionBody::Block(Rc::new(body?)),
            is_arrow: false,
        }))
    }

    fn parameter_list(&mut self) -> Result<(Vec<Pattern>, Option<String>), SyntaxError> {
        self.expect_punct("(")?;
        let mut params = Vec::new();
        let mut rest = None;
        while !self.peek().is_punct(")") {
            if self.eat_punct("...") {
                rest = Some(self.expect_binding_name()?);
            } else {
                let mut pattern = self.binding_pattern()?;
                if self.eat_punct("=") {
                    let default = self.assignment_expression()?;
                    pattern = Pattern::Default(Box::new(pattern), Box::new(default));
                }
                params.push(pattern);
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        Ok((params, rest))
    }

    fn class(&mut self) -> Result<Rc<Class>, SyntaxError> {
        self.advance(); // `class`
        let name = if matches!(self.peek().kind, TokenKind::Ident(_)) {
            Some(self.expect_binding_name()?)
        } else {
            None
        };
        let superclass = if self.eat_keyword("extends") {
            Some(Box::new(self.unary_or_higher()?))
        } else {
            None
        };
        self.expect_punct("{")?;

        let mut members = Vec::new();
        while !self.peek().is_punct("}") {
            if self.peek().is_eof() {
                return Err(self.error("unterminated class body"));
            }
            if self.eat_punct(";") {
                continue;
            }
            let is_static = self.peek().is_keyword("static")
                && !self.peek_at(1).is_punct("(")
                && !self.peek_at(1).is_punct("=");
            if is_static {
                self.advance();
            }
            // `get name()` and `set name(value)` define accessors. `get` is only
            // the keyword when a property name follows it; `get()` and `get = 1`
            // are an ordinary method and field of that name.
            let accessor = match &self.peek().kind {
                TokenKind::Ident(name)
                    if (name == "get" || name == "set")
                        && !self.peek_at(1).is_punct("(")
                        && !self.peek_at(1).is_punct("=")
                        && !self.peek_at(1).is_punct(";")
                        && !self.peek_at(1).is_punct("}") =>
                {
                    let kind = if name == "get" {
                        MemberKind::Getter
                    } else {
                        MemberKind::Setter
                    };
                    self.advance();
                    Some(kind)
                }
                _ => None,
            };
            let _ = self.eat_punct("*");
            let key = self.property_key()?;

            if self.peek().is_punct("(") {
                let (params, rest) = self.parameter_list()?;
                self.function_depth += 1;
                let body = self.block();
                self.function_depth -= 1;
                let is_constructor = !is_static
                    && accessor.is_none()
                    && key.static_name().as_deref() == Some("constructor");
                members.push(ClassMember {
                    kind: match accessor {
                        Some(kind) => kind,
                        None if is_constructor => MemberKind::Constructor,
                        None => MemberKind::Method,
                    },
                    is_static,
                    function: Some(Rc::new(Function {
                        name: key.static_name(),
                        params,
                        rest,
                        body: FunctionBody::Block(Rc::new(body?)),
                        is_arrow: false,
                    })),
                    value: None,
                    key,
                });
                continue;
            }

            // A field, with or without an initialiser.
            let value = if self.eat_punct("=") {
                Some(self.assignment_expression()?)
            } else {
                None
            };
            self.consume_semicolon()?;
            members.push(ClassMember {
                key,
                kind: MemberKind::Field,
                is_static,
                function: None,
                value,
            });
        }
        self.expect_punct("}")?;
        Ok(Rc::new(Class {
            name,
            superclass,
            members,
        }))
    }

    fn property_key(&mut self) -> Result<PropKey, SyntaxError> {
        match self.peek().kind.clone() {
            TokenKind::Str(text) => {
                self.advance();
                Ok(PropKey::Str(text))
            }
            TokenKind::Number(number) => {
                self.advance();
                Ok(PropKey::Number(number))
            }
            TokenKind::Punct("[") => {
                self.advance();
                let expr = self.assignment_expression()?;
                self.expect_punct("]")?;
                Ok(PropKey::Computed(expr))
            }
            // A private field name; treated as an ordinary property.
            TokenKind::Punct("#") => {
                self.advance();
                Ok(PropKey::Ident(format!("#{}", self.expect_name()?)))
            }
            _ => Ok(PropKey::Ident(self.expect_name()?)),
        }
    }

    // ---- expressions ------------------------------------------------------

    /// The comma operator, the lowest precedence.
    pub fn expression(&mut self) -> Result<Expr, SyntaxError> {
        let first = self.assignment_expression()?;
        if !self.peek().is_punct(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_punct(",") {
            items.push(self.assignment_expression()?);
        }
        Ok(Expr::Sequence(items))
    }

    fn assignment_expression(&mut self) -> Result<Expr, SyntaxError> {
        if let Some(arrow) = self.try_arrow_function()? {
            return Ok(arrow);
        }

        let left = self.conditional_expression()?;

        let compound = match &self.peek().kind {
            TokenKind::Punct("=") => Some(None),
            TokenKind::Punct("+=") => Some(Some(BinaryOp::Add)),
            TokenKind::Punct("-=") => Some(Some(BinaryOp::Subtract)),
            TokenKind::Punct("*=") => Some(Some(BinaryOp::Multiply)),
            TokenKind::Punct("/=") => Some(Some(BinaryOp::Divide)),
            TokenKind::Punct("%=") => Some(Some(BinaryOp::Remainder)),
            TokenKind::Punct("**=") => Some(Some(BinaryOp::Exponent)),
            TokenKind::Punct("&=") => Some(Some(BinaryOp::BitAnd)),
            TokenKind::Punct("|=") => Some(Some(BinaryOp::BitOr)),
            TokenKind::Punct("^=") => Some(Some(BinaryOp::BitXor)),
            TokenKind::Punct("<<=") => Some(Some(BinaryOp::ShiftLeft)),
            TokenKind::Punct(">>=") => Some(Some(BinaryOp::ShiftRight)),
            TokenKind::Punct(">>>=") => Some(Some(BinaryOp::ShiftRightUnsigned)),
            _ => None,
        };
        if let Some(op) = compound {
            self.advance();
            let value = self.assignment_expression()?;
            let target = left
                .into_pattern()
                .ok_or_else(|| self.error("invalid assignment target"))?;
            return Ok(Expr::Assign {
                op,
                target: Box::new(target),
                value: Box::new(value),
            });
        }

        let logical = match &self.peek().kind {
            TokenKind::Punct("&&=") => Some(LogicalOp::And),
            TokenKind::Punct("||=") => Some(LogicalOp::Or),
            TokenKind::Punct("??=") => Some(LogicalOp::Nullish),
            _ => None,
        };
        if let Some(op) = logical {
            self.advance();
            let value = self.assignment_expression()?;
            let target = left
                .into_pattern()
                .ok_or_else(|| self.error("invalid assignment target"))?;
            return Ok(Expr::LogicalAssign {
                op,
                target: Box::new(target),
                value: Box::new(value),
            });
        }

        Ok(left)
    }

    /// Parses an arrow function if one starts here.
    fn try_arrow_function(&mut self) -> Result<Option<Expr>, SyntaxError> {
        let start = self.pos;
        let is_async = self.at_async_function() && !self.peek_at(1).is_keyword("function");
        let offset = usize::from(is_async);

        // `x => …`
        if matches!(self.peek_at(offset).kind, TokenKind::Ident(_))
            && self.peek_at(offset + 1).is_punct("=>")
        {
            self.pos += offset;
            let name = self.expect_binding_name()?;
            self.expect_punct("=>")?;
            let body = self.arrow_body()?;
            return Ok(Some(Expr::Function(Rc::new(Function {
                name: None,
                params: vec![Pattern::Ident(name)],
                rest: None,
                body,
                is_arrow: true,
            }))));
        }

        // `(a, b) => …`
        if self.peek_at(offset).is_punct("(") && self.parenthesised_group_is_arrow(offset) {
            self.pos += offset;
            let (params, rest) = self.parameter_list()?;
            self.expect_punct("=>")?;
            let body = self.arrow_body()?;
            return Ok(Some(Expr::Function(Rc::new(Function {
                name: None,
                params,
                rest,
                body,
                is_arrow: true,
            }))));
        }

        self.pos = start;
        Ok(None)
    }

    /// Scans from the `(` at `offset` to its match, and reports whether `=>`
    /// follows. This is the lookahead that distinguishes a parameter list from a
    /// parenthesised expression.
    fn parenthesised_group_is_arrow(&self, offset: usize) -> bool {
        let mut depth = 0usize;
        let mut index = offset;
        loop {
            let token = self.peek_at(index);
            if token.is_eof() {
                return false;
            }
            match &token.kind {
                TokenKind::Punct("(") | TokenKind::Punct("[") | TokenKind::Punct("{") => depth += 1,
                TokenKind::Punct(")") | TokenKind::Punct("]") | TokenKind::Punct("}") => {
                    depth -= 1;
                    if depth == 0 {
                        return self.peek_at(index + 1).is_punct("=>");
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }

    fn arrow_body(&mut self) -> Result<FunctionBody, SyntaxError> {
        if self.peek().is_punct("{") {
            self.function_depth += 1;
            let body = self.block();
            self.function_depth -= 1;
            return Ok(FunctionBody::Block(Rc::new(body?)));
        }
        let expr = self.assignment_expression()?;
        Ok(FunctionBody::Expr(Box::new(expr)))
    }

    fn conditional_expression(&mut self) -> Result<Expr, SyntaxError> {
        let test = self.binary_expression(0)?;
        if !self.eat_punct("?") {
            return Ok(test);
        }
        let consequent = self.assignment_expression()?;
        self.expect_punct(":")?;
        let alternate = self.assignment_expression()?;
        Ok(Expr::Conditional {
            test: Box::new(test),
            consequent: Box::new(consequent),
            alternate: Box::new(alternate),
        })
    }

    /// Precedence climbing over the binary and logical operators.
    fn binary_expression(&mut self, min_precedence: u8) -> Result<Expr, SyntaxError> {
        let mut left = self.unary_or_higher()?;

        while let Some((precedence, right_associative, op)) = self.binary_operator() {
            if precedence < min_precedence {
                break;
            }
            self.advance();
            let next_min = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let right = self.binary_expression(next_min)?;
            left = match op {
                Operator::Binary(op) => Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                Operator::Logical(op) => Expr::Logical {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    /// The operator at the cursor, with its precedence and associativity.
    fn binary_operator(&self) -> Option<(u8, bool, Operator)> {
        use BinaryOp::*;
        let (precedence, op) = match &self.peek().kind {
            TokenKind::Punct("??") => (1, Operator::Logical(LogicalOp::Nullish)),
            TokenKind::Punct("||") => (2, Operator::Logical(LogicalOp::Or)),
            TokenKind::Punct("&&") => (3, Operator::Logical(LogicalOp::And)),
            TokenKind::Punct("|") => (4, Operator::Binary(BitOr)),
            TokenKind::Punct("^") => (5, Operator::Binary(BitXor)),
            TokenKind::Punct("&") => (6, Operator::Binary(BitAnd)),
            TokenKind::Punct("==") => (7, Operator::Binary(Equal)),
            TokenKind::Punct("!=") => (7, Operator::Binary(NotEqual)),
            TokenKind::Punct("===") => (7, Operator::Binary(StrictEqual)),
            TokenKind::Punct("!==") => (7, Operator::Binary(StrictNotEqual)),
            TokenKind::Punct("<") => (8, Operator::Binary(Less)),
            TokenKind::Punct("<=") => (8, Operator::Binary(LessEqual)),
            TokenKind::Punct(">") => (8, Operator::Binary(Greater)),
            TokenKind::Punct(">=") => (8, Operator::Binary(GreaterEqual)),
            TokenKind::Keyword("in") => (8, Operator::Binary(In)),
            TokenKind::Keyword("instanceof") => (8, Operator::Binary(InstanceOf)),
            TokenKind::Punct("<<") => (9, Operator::Binary(ShiftLeft)),
            TokenKind::Punct(">>") => (9, Operator::Binary(ShiftRight)),
            TokenKind::Punct(">>>") => (9, Operator::Binary(ShiftRightUnsigned)),
            TokenKind::Punct("+") => (10, Operator::Binary(Add)),
            TokenKind::Punct("-") => (10, Operator::Binary(Subtract)),
            TokenKind::Punct("*") => (11, Operator::Binary(Multiply)),
            TokenKind::Punct("/") => (11, Operator::Binary(Divide)),
            TokenKind::Punct("%") => (11, Operator::Binary(Remainder)),
            TokenKind::Punct("**") => return Some((12, true, Operator::Binary(Exponent))),
            _ => return None,
        };
        Some((precedence, false, op))
    }

    fn unary_or_higher(&mut self) -> Result<Expr, SyntaxError> {
        let op = match &self.peek().kind {
            TokenKind::Punct("-") => Some(UnaryOp::Negate),
            TokenKind::Punct("+") => Some(UnaryOp::Plus),
            TokenKind::Punct("!") => Some(UnaryOp::Not),
            TokenKind::Punct("~") => Some(UnaryOp::BitNot),
            TokenKind::Keyword("typeof") => Some(UnaryOp::TypeOf),
            TokenKind::Keyword("void") => Some(UnaryOp::Void),
            TokenKind::Keyword("delete") => Some(UnaryOp::Delete),
            TokenKind::Keyword("await") => Some(UnaryOp::Await),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let operand = self.unary_or_higher()?;
            return Ok(Expr::Unary {
                op,
                operand: Box::new(operand),
            });
        }

        // Prefix increment and decrement.
        for (text, op) in [("++", UpdateOp::Increment), ("--", UpdateOp::Decrement)] {
            if self.peek().is_punct(text) {
                self.advance();
                let target = self.unary_or_higher()?;
                if !target.is_assignable() {
                    return Err(self.error("invalid `++`/`--` target"));
                }
                return Ok(Expr::Update {
                    op,
                    prefix: true,
                    target: Box::new(target),
                });
            }
        }

        let expr = self.call_or_member_expression()?;

        // Postfix increment and decrement, which a line break separates.
        if !self.peek().newline_before {
            for (text, op) in [("++", UpdateOp::Increment), ("--", UpdateOp::Decrement)] {
                if self.peek().is_punct(text) {
                    self.advance();
                    if !expr.is_assignable() {
                        return Err(self.error("invalid `++`/`--` target"));
                    }
                    return Ok(Expr::Update {
                        op,
                        prefix: false,
                        target: Box::new(expr),
                    });
                }
            }
        }
        Ok(expr)
    }

    fn call_or_member_expression(&mut self) -> Result<Expr, SyntaxError> {
        let mut expr = if self.peek().is_keyword("new") {
            self.new_expression()?
        } else {
            self.primary_expression()?
        };

        loop {
            if self.eat_punct(".") {
                let name = self.expect_name()?;
                expr = Expr::MemberAccess {
                    object: Box::new(expr),
                    property: Member::Ident(name),
                    optional: false,
                };
            } else if self.peek().is_punct("?.") {
                self.advance();
                if self.peek().is_punct("(") {
                    let args = self.arguments()?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        optional: true,
                    };
                } else if self.eat_punct("[") {
                    let property = self.expression()?;
                    self.expect_punct("]")?;
                    expr = Expr::MemberAccess {
                        object: Box::new(expr),
                        property: Member::Computed(Box::new(property)),
                        optional: true,
                    };
                } else {
                    let name = self.expect_name()?;
                    expr = Expr::MemberAccess {
                        object: Box::new(expr),
                        property: Member::Ident(name),
                        optional: true,
                    };
                }
            } else if self.eat_punct("[") {
                let property = self.expression()?;
                self.expect_punct("]")?;
                expr = Expr::MemberAccess {
                    object: Box::new(expr),
                    property: Member::Computed(Box::new(property)),
                    optional: false,
                };
            } else if self.peek().is_punct("(") {
                let args = self.arguments()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    optional: false,
                };
            } else if matches!(self.peek().kind, TokenKind::Template(_)) {
                // A tagged template is called with the assembled string.
                let template = self.primary_expression()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args: vec![Argument::Normal(template)],
                    optional: false,
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn new_expression(&mut self) -> Result<Expr, SyntaxError> {
        self.advance(); // `new`
                        // `new.target` is not supported; treat it as undefined.
        if self.peek().is_punct(".") {
            self.advance();
            let _ = self.expect_name()?;
            return Ok(Expr::Undefined);
        }
        let mut callee = self.primary_expression()?;
        // Member accesses bind tighter than the call: `new a.b.C()`.
        loop {
            if self.eat_punct(".") {
                let name = self.expect_name()?;
                callee = Expr::MemberAccess {
                    object: Box::new(callee),
                    property: Member::Ident(name),
                    optional: false,
                };
            } else if self.eat_punct("[") {
                let property = self.expression()?;
                self.expect_punct("]")?;
                callee = Expr::MemberAccess {
                    object: Box::new(callee),
                    property: Member::Computed(Box::new(property)),
                    optional: false,
                };
            } else {
                break;
            }
        }
        let args = if self.peek().is_punct("(") {
            self.arguments()?
        } else {
            Vec::new()
        };
        Ok(Expr::New {
            callee: Box::new(callee),
            args,
        })
    }

    fn arguments(&mut self) -> Result<Vec<Argument>, SyntaxError> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        while !self.peek().is_punct(")") {
            if self.eat_punct("...") {
                args.push(Argument::Spread(self.assignment_expression()?));
            } else {
                args.push(Argument::Normal(self.assignment_expression()?));
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    fn primary_expression(&mut self) -> Result<Expr, SyntaxError> {
        match self.peek().kind.clone() {
            TokenKind::Number(number) => {
                self.advance();
                Ok(Expr::Number(number))
            }
            TokenKind::Str(text) => {
                self.advance();
                Ok(Expr::Str(text))
            }
            TokenKind::Template(pieces) => {
                self.advance();
                let mut elements = Vec::new();
                for piece in pieces {
                    match piece {
                        TemplatePiece::Text(text) => elements.push(TemplateElem::Text(text)),
                        TemplatePiece::Expr(source) => {
                            let expr = parse_expression(&source)?;
                            elements.push(TemplateElem::Expr(expr));
                        }
                    }
                }
                Ok(Expr::Template(elements))
            }
            TokenKind::Keyword("true") => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            TokenKind::Keyword("false") => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            TokenKind::Keyword("null") => {
                self.advance();
                Ok(Expr::Null)
            }
            TokenKind::Keyword("undefined") => {
                self.advance();
                Ok(Expr::Undefined)
            }
            TokenKind::Keyword("this") => {
                self.advance();
                Ok(Expr::This)
            }
            TokenKind::Keyword("super") => {
                self.advance();
                Ok(Expr::Super)
            }
            TokenKind::Keyword("function") => {
                let function = self.function(false)?;
                Ok(Expr::Function(function))
            }
            TokenKind::Keyword("class") => {
                let class = self.class()?;
                Ok(Expr::Class(class))
            }
            TokenKind::Punct("(") => {
                self.advance();
                let expr = self.expression()?;
                self.expect_punct(")")?;
                Ok(expr)
            }
            TokenKind::Punct("[") => self.array_literal(),
            TokenKind::Punct("{") => self.object_literal(),
            TokenKind::Ident(_) => {
                if self.at_async_function() {
                    self.advance();
                    if self.peek().is_keyword("function") {
                        let function = self.function(false)?;
                        return Ok(Expr::Function(function));
                    }
                }
                let name = self.expect_binding_name()?;
                Ok(Expr::Ident(name))
            }
            TokenKind::Eof => Err(self.error("unexpected end of input")),
            other => Err(self.error(format!("unexpected token {other:?}"))),
        }
    }

    fn array_literal(&mut self) -> Result<Expr, SyntaxError> {
        self.expect_punct("[")?;
        let mut elements = Vec::new();
        while !self.peek().is_punct("]") {
            if self.peek().is_punct(",") {
                self.advance();
                elements.push(ArrayElem::Hole);
                continue;
            }
            if self.eat_punct("...") {
                elements.push(ArrayElem::Spread(self.assignment_expression()?));
            } else {
                elements.push(ArrayElem::Item(self.assignment_expression()?));
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct("]")?;
        Ok(Expr::Array(elements))
    }

    fn object_literal(&mut self) -> Result<Expr, SyntaxError> {
        self.expect_punct("{")?;
        let mut props = Vec::new();
        while !self.peek().is_punct("}") {
            if self.peek().is_eof() {
                return Err(self.error("unterminated object literal"));
            }
            if self.eat_punct("...") {
                props.push(ObjectProp::Spread(self.assignment_expression()?));
                if !self.eat_punct(",") {
                    break;
                }
                continue;
            }

            // `get x() {}` and `set x(v) {}` define accessors. A property that is
            // merely *named* `get` — `{ get: 1 }`, `{ get }`, `{ get() {} }` — is
            // not one.
            let accessor = match &self.peek().kind {
                TokenKind::Ident(name)
                    if (name == "get" || name == "set")
                        && !self.peek_at(1).is_punct(":")
                        && !self.peek_at(1).is_punct(",")
                        && !self.peek_at(1).is_punct("(")
                        && !self.peek_at(1).is_punct("}") =>
                {
                    let is_getter = name == "get";
                    self.advance();
                    Some(is_getter)
                }
                _ => None,
            };
            let is_async = self.at_async_function();
            if is_async {
                self.advance();
            }
            let _ = self.eat_punct("*");

            let key = self.property_key()?;
            if self.peek().is_punct("(") {
                let (params, rest) = self.parameter_list()?;
                self.function_depth += 1;
                let body = self.block();
                self.function_depth -= 1;
                let function = Rc::new(Function {
                    name: key.static_name(),
                    params,
                    rest,
                    body: FunctionBody::Block(Rc::new(body?)),
                    is_arrow: false,
                });
                props.push(match accessor {
                    Some(true) => ObjectProp::Getter { key, function },
                    Some(false) => ObjectProp::Setter { key, function },
                    None => ObjectProp::Method { key, function },
                });
            } else if self.eat_punct(":") {
                let value = self.assignment_expression()?;
                props.push(ObjectProp::KeyValue { key, value });
            } else {
                // Shorthand: `{ a }`, and `{ a = 1 }` inside a pattern.
                let name = key
                    .static_name()
                    .ok_or_else(|| self.error("a computed key needs a value"))?;
                let mut value = Expr::Ident(name);
                if self.eat_punct("=") {
                    let default = self.assignment_expression()?;
                    let target = value
                        .into_pattern()
                        .ok_or_else(|| self.error("invalid shorthand default"))?;
                    value = Expr::Assign {
                        op: None,
                        target: Box::new(target),
                        value: Box::new(default),
                    };
                }
                props.push(ObjectProp::KeyValue { key, value });
            }

            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct("}")?;
        Ok(Expr::Object(props))
    }
}

enum Operator {
    Binary(BinaryOp),
    Logical(LogicalOp),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(source: &str) -> Program {
        parse(source).unwrap_or_else(|error| panic!("failed to parse {source:?}: {error}"))
    }

    fn expr(source: &str) -> Expr {
        match &program(source).body[0] {
            Stmt::Expr(expr) => expr.clone(),
            other => panic!("expected an expression statement, got {other:?}"),
        }
    }

    #[test]
    fn literals_and_identifiers() {
        assert_eq!(expr("1"), Expr::Number(1.0));
        assert_eq!(expr("'a'"), Expr::Str("a".into()));
        assert_eq!(expr("true"), Expr::Bool(true));
        assert_eq!(expr("null"), Expr::Null);
        assert_eq!(expr("undefined"), Expr::Undefined);
        assert_eq!(expr("x"), Expr::Ident("x".into()));
        assert_eq!(expr("this"), Expr::This);
    }

    #[test]
    fn operator_precedence_is_respected() {
        // 1 + 2 * 3 groups as 1 + (2 * 3).
        match expr("1 + 2 * 3") {
            Expr::Binary {
                op: BinaryOp::Add,
                right,
                ..
            } => assert!(matches!(
                *right,
                Expr::Binary {
                    op: BinaryOp::Multiply,
                    ..
                }
            )),
            other => panic!("got {other:?}"),
        }
        // Parentheses override it.
        match expr("(1 + 2) * 3") {
            Expr::Binary {
                op: BinaryOp::Multiply,
                left,
                ..
            } => assert!(matches!(
                *left,
                Expr::Binary {
                    op: BinaryOp::Add,
                    ..
                }
            )),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn exponentiation_is_right_associative() {
        match expr("2 ** 3 ** 2") {
            Expr::Binary {
                op: BinaryOp::Exponent,
                right,
                ..
            } => assert!(matches!(
                *right,
                Expr::Binary {
                    op: BinaryOp::Exponent,
                    ..
                }
            )),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn logical_and_binds_tighter_than_or() {
        match expr("a || b && c") {
            Expr::Logical {
                op: LogicalOp::Or,
                right,
                ..
            } => assert!(matches!(
                *right,
                Expr::Logical {
                    op: LogicalOp::And,
                    ..
                }
            )),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn assignment_forms() {
        assert!(matches!(expr("a = 1"), Expr::Assign { op: None, .. }));
        assert!(matches!(
            expr("a += 1"),
            Expr::Assign {
                op: Some(BinaryOp::Add),
                ..
            }
        ));
        assert!(matches!(
            expr("a ??= 1"),
            Expr::LogicalAssign {
                op: LogicalOp::Nullish,
                ..
            }
        ));
        assert!(parse("1 = 2").is_err(), "cannot assign to a literal");
    }

    #[test]
    fn member_and_call_chains() {
        match expr("a.b[c](d)") {
            Expr::Call { callee, args, .. } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    *callee,
                    Expr::MemberAccess {
                        property: Member::Computed(_),
                        ..
                    }
                ));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn optional_chaining() {
        assert!(matches!(
            expr("a?.b"),
            Expr::MemberAccess { optional: true, .. }
        ));
        assert!(matches!(expr("a?.()"), Expr::Call { optional: true, .. }));
        assert!(matches!(
            expr("a?.[0]"),
            Expr::MemberAccess { optional: true, .. }
        ));
    }

    #[test]
    fn arrow_functions_in_every_shape() {
        for source in [
            "x => x",
            "(x) => x",
            "(a, b) => a + b",
            "() => 1",
            "x => { return x }",
        ] {
            match expr(source) {
                Expr::Function(function) => assert!(function.is_arrow, "{source}"),
                other => panic!("{source} produced {other:?}"),
            }
        }
    }

    #[test]
    fn a_parenthesised_expression_is_not_an_arrow() {
        assert!(matches!(expr("(a + b)"), Expr::Binary { .. }));
        assert!(matches!(expr("(a, b)"), Expr::Sequence(_)));
    }

    #[test]
    fn arrow_bodies_may_be_object_literals() {
        match expr("() => ({ a: 1 })") {
            Expr::Function(function) => match &function.body {
                FunctionBody::Expr(body) => assert!(matches!(**body, Expr::Object(_))),
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn function_declarations_and_expressions() {
        match &program("function f(a, b = 2, ...rest) {}").body[0] {
            Stmt::Function(function) => {
                assert_eq!(function.name.as_deref(), Some("f"));
                assert_eq!(function.params.len(), 2);
                assert!(matches!(function.params[1], Pattern::Default(_, _)));
                assert_eq!(function.rest.as_deref(), Some("rest"));
            }
            other => panic!("got {other:?}"),
        }
        assert!(matches!(expr("(function () {})"), Expr::Function(_)));
    }

    #[test]
    fn object_and_array_literals() {
        // The parentheses are required: at the start of a statement a brace
        // opens a block, exactly as in a real engine.
        match expr("({ a: 1, b, 'c': 3, [k]: 4, m() {}, ...rest })") {
            Expr::Object(props) => assert_eq!(props.len(), 6),
            other => panic!("got {other:?}"),
        }
        match expr("[1, , 3, ...rest]") {
            Expr::Array(items) => {
                assert_eq!(items.len(), 4);
                assert_eq!(items[1], ArrayElem::Hole);
                assert!(matches!(items[3], ArrayElem::Spread(_)));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn template_literals_parse_their_substitutions() {
        match expr("`a ${1 + 2} b`") {
            Expr::Template(elements) => {
                assert_eq!(elements.len(), 3);
                assert!(matches!(
                    elements[1],
                    TemplateElem::Expr(Expr::Binary { .. })
                ));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn destructuring_declarations() {
        match &program("const { a, b: c, ...rest } = obj;").body[0] {
            Stmt::VarDecl { declarations, .. } => match &declarations[0].0 {
                Pattern::Object { props, rest } => {
                    assert_eq!(props.len(), 2);
                    assert_eq!(rest.as_deref(), Some("rest"));
                }
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }

        match &program("let [x, , y = 2, ...more] = list;").body[0] {
            Stmt::VarDecl { declarations, .. } => match &declarations[0].0 {
                Pattern::Array { items, rest } => {
                    assert_eq!(items.len(), 3);
                    assert!(items[1].is_none());
                    assert!(matches!(items[2], Some(Pattern::Default(_, _))));
                    assert!(rest.is_some());
                }
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn destructuring_assignment_recovers_a_pattern() {
        match expr("({ a } = obj)") {
            Expr::Assign { target, .. } => {
                assert!(matches!(*target, Pattern::Object { .. }))
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn control_flow_statements() {
        assert!(matches!(
            program("if (a) b; else c;").body[0],
            Stmt::If { .. }
        ));
        assert!(matches!(
            program("while (a) b;").body[0],
            Stmt::While { .. }
        ));
        assert!(matches!(
            program("do b; while (a)").body[0],
            Stmt::DoWhile { .. }
        ));
        assert!(matches!(
            program("for (let i = 0; i < 3; i++) {}").body[0],
            Stmt::For { .. }
        ));
        assert!(matches!(
            program("for (const x of xs) {}").body[0],
            Stmt::ForOf { .. }
        ));
        assert!(matches!(
            program("for (const k in o) {}").body[0],
            Stmt::ForIn { .. }
        ));
        assert!(matches!(
            program("for (;;) break;").body[0],
            Stmt::For { .. }
        ));
    }

    #[test]
    fn a_for_loop_may_declare_several_names() {
        match &program("for (let i = 0, j = 10; i < j; i++, j--) {}").body[0] {
            Stmt::For { init, .. } => match init.as_deref() {
                Some(Stmt::VarDecl { declarations, .. }) => assert_eq!(declarations.len(), 2),
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn try_catch_finally() {
        match &program("try { a() } catch (e) { b(e) } finally { c() }").body[0] {
            Stmt::Try {
                param,
                handler,
                finalizer,
                ..
            } => {
                assert!(param.is_some());
                assert!(handler.is_some());
                assert!(finalizer.is_some());
            }
            other => panic!("got {other:?}"),
        }
        // An optional catch binding.
        assert!(parse("try { a() } catch { b() }").is_ok());
        assert!(parse("try { a() }").is_err(), "needs catch or finally");
    }

    #[test]
    fn switch_statements() {
        match &program("switch (x) { case 1: a(); break; default: b() }").body[0] {
            Stmt::Switch { cases, .. } => {
                assert_eq!(cases.len(), 2);
                assert!(cases[0].test.is_some());
                assert!(cases[1].test.is_none());
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn classes_with_methods_fields_and_inheritance() {
        match &program(
            "class A extends B { static count = 0; x = 1; constructor(v) { super(v) } go() {} }",
        )
        .body[0]
        {
            Stmt::Class(class) => {
                assert_eq!(class.name.as_deref(), Some("A"));
                assert!(class.superclass.is_some());
                assert_eq!(class.members.len(), 4);
                assert!(class.members[0].is_static);
                assert_eq!(class.members[2].kind, MemberKind::Constructor);
                assert_eq!(class.members[3].kind, MemberKind::Method);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn labelled_statements_and_labelled_break() {
        match &program("outer: for (;;) { break outer; }").body[0] {
            Stmt::Labeled { label, .. } => assert_eq!(label, "outer"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn semicolons_are_inserted_at_line_breaks() {
        let parsed = program("let a = 1\nlet b = 2\na + b");
        assert_eq!(parsed.body.len(), 3);
    }

    #[test]
    fn return_stops_at_a_line_break() {
        match &program("function f() { return\n1 }").body[0] {
            Stmt::Function(function) => match &function.body {
                FunctionBody::Block(body) => {
                    assert_eq!(body.len(), 2);
                    assert!(matches!(body[0], Stmt::Return(None)));
                }
                other => panic!("got {other:?}"),
            },
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_return_outside_a_function_is_an_error() {
        assert!(parse("return 1").is_err());
        assert!(parse("function f() { return 1 }").is_ok());
    }

    #[test]
    fn keywords_are_allowed_as_property_names() {
        assert!(parse("a.new; a.class; ({ if: 1, for: 2 })").is_ok());
    }

    #[test]
    fn new_expressions() {
        match expr("new a.b.C(1)") {
            Expr::New { callee, args } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(*callee, Expr::MemberAccess { .. }));
            }
            other => panic!("got {other:?}"),
        }
        assert!(matches!(expr("new Thing"), Expr::New { .. }));
    }

    #[test]
    fn async_and_await_are_accepted() {
        assert!(parse("async function f() { const x = await g(); return x }").is_ok());
        assert!(parse("const f = async () => await g()").is_ok());
        // `async` is still usable as a plain name.
        assert!(parse("let async = 1; async + 1").is_ok());
    }

    #[test]
    fn syntax_errors_carry_a_line_number() {
        let error = parse("let a = 1;\nlet = ;").expect_err("should fail");
        assert_eq!(error.line, 2);

        assert!(parse("function f( {").is_err());
        assert!(parse("if (").is_err());
        assert!(parse("{ unterminated").is_err());
    }

    #[test]
    fn deeply_nested_expressions_parse() {
        let source = format!("let x = {}1{};", "(".repeat(50), ")".repeat(50));
        assert!(parse(&source).is_ok());
    }

    #[test]
    fn a_realistic_script_parses() {
        let source = r#"
            const items = [1, 2, 3].map(n => n * 2).filter(n => n > 2);
            let total = 0;
            for (const n of items) { total += n }

            class Counter {
                constructor(start = 0) { this.value = start }
                add(n) { this.value += n; return this }
                get() { return this.value }
            }

            const c = new Counter(total);
            c.add(1).add(2);

            const { value } = c;
            const label = `total is ${value}`;

            function describe(thing) {
                switch (typeof thing) {
                    case 'number': return 'a number';
                    case 'string': return 'a string';
                    default: return 'something else';
                }
            }

            try {
                if (!value) throw new Error('empty');
            } catch (error) {
                describe(error);
            } finally {
                describe(label);
            }
        "#;
        let parsed = program(source);
        assert!(parsed.body.len() >= 8);
    }
}
