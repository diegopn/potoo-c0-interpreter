use std::{collections::HashMap, iter::Peekable, vec::IntoIter};

use crate::{
    Diagnostic,
    lexer::{Token, TokenKind},
    syntax::{
        Binding, Contract, ContractKind, Declaration, Expression, ExpressionKind, Function,
        Literal, Statement, StatementKind, Type,
    },
};

pub(crate) fn parse(
    tokens: Peekable<IntoIter<Token>>,
    aliases: &mut HashMap<String, Type>,
    native: bool,
) -> Result<Vec<Declaration>, Diagnostic> {
    Parser { tokens, aliases }.program(native)
}

struct Parser<'a> {
    tokens: Peekable<IntoIter<Token>>,
    aliases: &'a mut HashMap<String, Type>,
}

impl Parser<'_> {
    fn current(&mut self) -> &Token {
        self.tokens.peek().expect("lexer appends an end token")
    }

    fn at(&mut self, expected: &str) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(symbol) if symbol == expected)
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self.at(expected) {
            self.tokens.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &str) -> Result<(), Diagnostic> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self
                .current()
                .location
                .error(format!("expected '{expected}'")))
        }
    }

    fn is_type(&mut self) -> bool {
        match &self.tokens.peek().expect("end token").kind {
            TokenKind::Type(_) | TokenKind::Symbol("struct") => true,
            TokenKind::Identifier(name) => self.aliases.contains_key(name),
            _ => false,
        }
    }

    fn identifier(&mut self) -> Result<String, Diagnostic> {
        match self
            .tokens
            .next_if(|token| matches!(token.kind, TokenKind::Identifier(_)))
        {
            Some(Token {
                kind: TokenKind::Identifier(name),
                ..
            }) => Ok(name),
            _ => Err(self.current().location.error("expected identifier")),
        }
    }

    fn ty(&mut self) -> Result<Type, Diagnostic> {
        let token = self
            .tokens
            .next_if(|token| !matches!(token.kind, TokenKind::End))
            .ok_or_else(|| self.current().location.error("expected type"))?;
        let mut ty = match token.kind {
            TokenKind::Type(ty) => ty,
            TokenKind::Symbol("struct") => Type::Struct(self.identifier()?),
            TokenKind::Identifier(name) => self
                .aliases
                .get(&name)
                .cloned()
                .ok_or_else(|| token.location.error(format!("unknown type '{name}'")))?,
            _ => return Err(token.location.error("expected type")),
        };

        loop {
            if self.consume("*") {
                ty = Type::Pointer(Box::new(ty));
            } else if self.consume("[") {
                self.expect("]")?;
                ty = Type::Array(Box::new(ty));
            } else {
                return Ok(ty);
            }
        }
    }

    fn binding(&mut self) -> Result<Binding, Diagnostic> {
        let location = self.current().location;
        let ty = self.ty()?;
        self.binding_of(location, ty)
    }

    fn binding_of(&mut self, location: crate::Location, ty: Type) -> Result<Binding, Diagnostic> {
        let name = self.identifier()?;
        Ok(Binding { location, name, ty })
    }

    fn program(&mut self, native: bool) -> Result<Vec<Declaration>, Diagnostic> {
        let mut declarations = Vec::new();
        while !matches!(self.current().kind, TokenKind::End) {
            let location = self.current().location;
            if self
                .tokens
                .next_if(|token| matches!(token.kind, TokenKind::IgnoredDirective))
                .is_some()
            {
                continue;
            }
            if matches!(self.current().kind, TokenKind::Import(_)) {
                return Err(location.error("#use directives must precede all declarations"));
            }
            if self.consume("typedef") {
                let alias = self.binding()?;
                self.expect(";")?;
                if self
                    .aliases
                    .insert(alias.name.clone(), alias.ty.clone())
                    .is_some()
                {
                    return Err(alias
                        .location
                        .error(format!("type '{}' is already defined", alias.name)));
                }
                declarations.push(Declaration::Alias(alias));
                continue;
            }

            let explicit_struct = self.at("struct");
            let ty = self.ty()?;
            if explicit_struct
                && let Type::Struct(name) = &ty
                && (self.at("{") || self.at(";"))
            {
                let fields = if self.consume("{") {
                    let mut fields = Vec::new();
                    while !self.consume("}") {
                        fields.push(self.binding()?);
                        self.expect(";")?;
                    }
                    Some(fields)
                } else {
                    None
                };
                self.expect(";")?;
                declarations.push(Declaration::Struct {
                    location,
                    name: name.clone(),
                    fields,
                    native,
                });
                continue;
            }

            let Binding {
                name, ty: result, ..
            } = self.binding_of(location, ty)?;
            self.expect("(")?;
            let parameters = self.comma_list(Self::binding)?;
            let contracts = self.annotations()?;
            let body = if self.consume(";") {
                None
            } else {
                self.expect("{")?;
                Some(self.block()?)
            };
            declarations.push(Declaration::Function(Function {
                location,
                name,
                result,
                parameters,
                contracts,
                body,
                native,
            }));
        }
        Ok(declarations)
    }

    fn annotations(&mut self) -> Result<Vec<Contract>, Diagnostic> {
        let mut annotations = Vec::new();
        while let Some(token) = self
            .tokens
            .next_if(|token| matches!(token.kind, TokenKind::Annotation(_)))
        {
            let TokenKind::Annotation(tokens) = token.kind else {
                unreachable!("annotation token")
            };
            let mut parser = Parser {
                tokens: tokens.into_iter().peekable(),
                aliases: self.aliases,
            };
            while !matches!(parser.current().kind, TokenKind::End) {
                let location = parser.current().location;
                let kind = match &parser.current().kind {
                    TokenKind::Symbol("assert") => ContractKind::Assert,
                    TokenKind::Identifier(marker) if marker == "requires" => ContractKind::Requires,
                    TokenKind::Identifier(marker) if marker == "ensures" => ContractKind::Ensures,
                    TokenKind::Identifier(marker) if marker == "loop_invariant" => {
                        ContractKind::Invariant
                    }
                    _ => {
                        return Err(
                            location.error("expected requires, ensures, loop_invariant, or assert")
                        );
                    }
                };
                parser.tokens.next();
                annotations.push(Contract {
                    kind,
                    expression: parser.expression(0)?,
                });
                parser.expect(";")?;
            }
        }
        Ok(annotations)
    }

    fn comma_list<T>(
        &mut self,
        item: impl Fn(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<Vec<T>, Diagnostic> {
        let mut items = Vec::new();
        if !self.consume(")") {
            loop {
                items.push(item(self)?);
                if self.consume(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        Ok(items)
    }

    fn block(&mut self) -> Result<Vec<Statement>, Diagnostic> {
        let mut statements = Vec::new();
        while !self.consume("}") {
            statements.push(self.statement(true)?);
        }
        Ok(statements)
    }

    fn statement(&mut self, allow_declaration: bool) -> Result<Statement, Diagnostic> {
        let location = self.current().location;
        let kind = if self.consume("{") {
            StatementKind::Block(self.block()?)
        } else if matches!(self.current().kind, TokenKind::Annotation(_)) {
            let contracts = self.annotations()?;
            if self.at("}") && allow_declaration {
                StatementKind::Contracts(contracts)
            } else {
                StatementKind::Guard(contracts, Box::new(self.statement(allow_declaration)?))
            }
        } else if self.consume("if") {
            let condition = self.parenthesized()?;
            let then_branch = Box::new(self.statement(false)?);
            let else_branch = if self.consume("else") {
                Some(Box::new(self.statement(false)?))
            } else {
                None
            };
            StatementKind::If(condition, then_branch, else_branch)
        } else if self.consume("while") {
            let condition = self.parenthesized()?;
            let invariants = self.annotations()?;
            StatementKind::Loop {
                initializer: None,
                condition,
                step: None,
                body: Box::new(self.statement(false)?),
                invariants,
            }
        } else if self.consume("for") {
            self.expect("(")?;
            let initializer = self.optional_simple(";", true)?;
            let condition = self.expression(0)?;
            self.expect(";")?;
            let step = self.optional_simple(")", false)?;
            let invariants = self.annotations()?;
            StatementKind::Loop {
                initializer,
                condition,
                step,
                body: Box::new(self.statement(false)?),
                invariants,
            }
        } else if self.consume("return") {
            let value = if self.at(";") {
                None
            } else {
                Some(self.expression(0)?)
            };
            self.expect(";")?;
            StatementKind::Return(value)
        } else if self.at("assert") || self.at("error") {
            let assertion = self.consume("assert");
            if !assertion {
                self.expect("error")?;
            }
            let argument = self.parenthesized()?;
            self.expect(";")?;
            if assertion {
                StatementKind::Assert(argument)
            } else {
                StatementKind::Error(argument)
            }
        } else {
            if self.at("break") || self.at("continue") {
                return Err(location.error("break and continue require C1"));
            }
            let statement = self.simple(allow_declaration)?;
            self.expect(";")?;
            return Ok(statement);
        };
        Ok(Statement { location, kind })
    }

    fn parenthesized(&mut self) -> Result<Expression, Diagnostic> {
        self.expect("(")?;
        let expression = self.expression(0)?;
        self.expect(")")?;
        Ok(expression)
    }

    fn optional_simple(
        &mut self,
        end: &str,
        allow_declaration: bool,
    ) -> Result<Option<Box<Statement>>, Diagnostic> {
        if self.consume(end) {
            return Ok(None);
        }
        let statement = self.simple(allow_declaration)?;
        self.expect(end)?;
        Ok(Some(Box::new(statement)))
    }

    fn simple(&mut self, allow_declaration: bool) -> Result<Statement, Diagnostic> {
        let location = self.current().location;
        if self.is_type() {
            if !allow_declaration {
                return Err(location.error("a declaration is not allowed here"));
            }
            let binding = self.binding()?;
            let initial = if self.consume("=") {
                Some(self.expression(0)?)
            } else {
                None
            };
            return Ok(Statement {
                location,
                kind: StatementKind::Declare(binding, initial),
            });
        }

        let expression = self.expression(0)?;
        let op = match self.current().kind {
            TokenKind::Symbol("=") => Some(None),
            TokenKind::Symbol(
                op @ ("+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "^=" | "|=" | "<<=" | ">>="),
            ) => Some(Some(
                op.strip_suffix('=')
                    .expect("compound assignment ends with '='"),
            )),
            TokenKind::Symbol("++") => Some(Some("+")),
            TokenKind::Symbol("--") => Some(Some("-")),
            _ => None,
        };

        let kind = match op {
            Some(op) => {
                if !expression.is_lvalue() {
                    return Err(
                        location.error("expected an lvalue on the left side of the assignment")
                    );
                }
                let operator_location = self.current().location;
                let postfix = self.at("++") || self.at("--");
                if postfix && matches!(expression.kind, ExpressionKind::Unary("*", _)) {
                    return Err(
                        location.error("parenthesize a dereference before increment or decrement")
                    );
                }
                self.tokens.next();
                let value = if postfix {
                    Expression {
                        location: operator_location,
                        kind: ExpressionKind::Literal(Literal::Int(1)),
                    }
                } else {
                    self.expression(0)?
                };
                return Ok(Statement {
                    location: operator_location,
                    kind: StatementKind::Assign(expression, op, value),
                });
            }
            None => StatementKind::Expression(expression),
        };
        Ok(Statement { location, kind })
    }

    fn expression(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        let mut expression = self.prefix()?;

        while let TokenKind::Symbol(operator) = self.current().kind {
            let precedence = match operator {
                "?" => 1,
                "||" => 2,
                "&&" => 3,
                "|" => 4,
                "^" => 5,
                "&" => 6,
                "==" | "!=" => 7,
                "<" | "<=" | ">" | ">=" => 8,
                "<<" | ">>" => 9,
                "+" | "-" => 10,
                "*" | "/" | "%" => 11,
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            let location = self.current().location;
            self.tokens.next();
            let kind = if operator == "?" {
                let then_branch = self.expression(0)?;
                self.expect(":")?;
                let else_branch = self.expression(precedence)?;
                ExpressionKind::Conditional(
                    Box::new(expression),
                    Box::new(then_branch),
                    Box::new(else_branch),
                )
            } else {
                let right = self.expression(precedence + 1)?;
                ExpressionKind::Binary(operator, Box::new(expression), Box::new(right))
            };
            expression = Expression { location, kind };
        }

        Ok(expression)
    }

    fn prefix(&mut self) -> Result<Expression, Diagnostic> {
        let location = self.current().location;
        let token = self
            .tokens
            .next_if(|token| !matches!(token.kind, TokenKind::End))
            .ok_or_else(|| location.error("expected expression"))?;
        let kind = match token.kind {
            TokenKind::Literal(value) => ExpressionKind::Literal(value),
            TokenKind::Identifier(name) if self.consume("(") => {
                ExpressionKind::Call(name, self.comma_list(|parser| parser.expression(0))?)
            }
            TokenKind::Identifier(name) => ExpressionKind::Variable(name),
            TokenKind::Symbol(operator @ ("!" | "-" | "~" | "*")) => {
                return Ok(Expression {
                    location,
                    kind: ExpressionKind::Unary(operator, Box::new(self.prefix()?)),
                });
            }
            TokenKind::Symbol("(") => {
                let expression = self.expression(0)?;
                self.expect(")")?;
                ExpressionKind::Group(Box::new(expression))
            }
            TokenKind::Symbol("NULL") => ExpressionKind::Literal(Literal::Null),
            TokenKind::Symbol(form @ ("alloc" | "alloc_array")) => {
                self.expect("(")?;
                let ty = self.ty()?;
                let size = if form == "alloc_array" {
                    self.expect(",")?;
                    Some(Box::new(self.expression(0)?))
                } else {
                    None
                };
                self.expect(")")?;
                ExpressionKind::Allocate(ty, size)
            }
            TokenKind::Symbol("\\result") => ExpressionKind::Result,
            TokenKind::Symbol("\\length") => {
                ExpressionKind::Length(Box::new(self.parenthesized()?))
            }
            _ => return Err(location.error("expected expression")),
        };
        let mut expression = Expression { location, kind };
        loop {
            let location = self.current().location;
            let kind = if self.consume("[") {
                let index = self.expression(0)?;
                self.expect("]")?;
                ExpressionKind::Index(Box::new(expression), Box::new(index))
            } else if self.consume(".") {
                ExpressionKind::Field(Box::new(expression), self.identifier()?)
            } else if self.consume("->") {
                let pointer = Expression {
                    location,
                    kind: ExpressionKind::Unary("*", Box::new(expression)),
                };
                ExpressionKind::Field(Box::new(pointer), self.identifier()?)
            } else {
                return Ok(expression);
            };
            expression = Expression { location, kind };
        }
    }
}
