use std::{fmt, sync::Arc};

use crate::Location;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Type {
    Int,
    Bool,
    Char,
    String,
    Void,
    Null,
    Struct(String),
    Pointer(Box<Type>),
    Array(Box<Type>),
}

impl Type {
    pub(crate) fn is_small(&self) -> bool {
        !matches!(self, Self::Struct(_) | Self::Void)
    }

    pub(crate) fn accepts(&self, actual: &Self) -> bool {
        self == actual || matches!((self, actual), (Self::Pointer(_), Self::Null))
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int => formatter.write_str("int"),
            Self::Bool => formatter.write_str("bool"),
            Self::Char => formatter.write_str("char"),
            Self::String => formatter.write_str("string"),
            Self::Void => formatter.write_str("void"),
            Self::Null => formatter.write_str("NULL"),
            Self::Struct(name) => write!(formatter, "struct {name}"),
            Self::Pointer(pointee) => write!(formatter, "{pointee}*"),
            Self::Array(element) => write!(formatter, "{element}[]"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Literal {
    Int(i32),
    Bool(bool),
    Char(u8),
    String(Arc<[u8]>),
    Null,
}

impl Literal {
    pub(crate) fn value_type(&self) -> Type {
        match self {
            Self::Int(_) => Type::Int,
            Self::Bool(_) => Type::Bool,
            Self::Char(_) => Type::Char,
            Self::String(_) => Type::String,
            Self::Null => Type::Null,
        }
    }
}

pub(crate) enum Declaration {
    Function(Function),
    Struct {
        location: Location,
        name: String,
        fields: Option<Vec<Binding>>,
    },
    Alias(Binding),
}

pub(crate) struct Function {
    pub(crate) location: Location,
    pub(crate) name: String,
    pub(crate) result: Type,
    pub(crate) parameters: Vec<Binding>,
    pub(crate) contracts: Vec<Contract>,
    pub(crate) body: Option<Vec<Statement>>,
    pub(crate) native: bool,
}

pub(crate) struct Binding {
    pub(crate) location: Location,
    pub(crate) name: String,
    pub(crate) ty: Type,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ContractKind {
    Requires,
    Ensures,
    Invariant,
    Assert,
}

pub(crate) struct Contract {
    pub(crate) kind: ContractKind,
    pub(crate) expression: Expression,
}

pub(crate) struct Expression {
    pub(crate) location: Location,
    pub(crate) kind: ExpressionKind,
}

pub(crate) enum ExpressionKind {
    Literal(Literal),
    Variable(String),
    Call(String, Vec<Expression>),
    Unary(&'static str, Box<Expression>),
    Binary(&'static str, Box<Expression>, Box<Expression>),
    Conditional(Box<Expression>, Box<Expression>, Box<Expression>),
    Group(Box<Expression>),
    Field(Box<Expression>, String),
    Index(Box<Expression>, Box<Expression>),
    Allocate(Type, Option<Box<Expression>>),
    Length(Box<Expression>),
    Result,
}

impl Expression {
    pub(crate) fn is_lvalue(&self) -> bool {
        match &self.kind {
            ExpressionKind::Variable(_) => true,
            ExpressionKind::Unary("*", base)
            | ExpressionKind::Group(base)
            | ExpressionKind::Field(base, _)
            | ExpressionKind::Index(base, _) => base.is_lvalue(),
            _ => false,
        }
    }

    pub(crate) fn visit(&self, operation: &mut impl FnMut(&Expression)) {
        operation(self);
        match &self.kind {
            ExpressionKind::Call(_, arguments) => {
                for argument in arguments {
                    argument.visit(operation);
                }
            }
            ExpressionKind::Unary(_, operand)
            | ExpressionKind::Group(operand)
            | ExpressionKind::Field(operand, _)
            | ExpressionKind::Length(operand)
            | ExpressionKind::Allocate(_, Some(operand)) => operand.visit(operation),
            ExpressionKind::Binary(_, left, right) | ExpressionKind::Index(left, right) => {
                left.visit(operation);
                right.visit(operation);
            }
            ExpressionKind::Conditional(condition, yes, no) => {
                condition.visit(operation);
                yes.visit(operation);
                no.visit(operation);
            }
            _ => {}
        }
    }

    pub(crate) fn variable(&self) -> Option<&str> {
        match &self.kind {
            ExpressionKind::Variable(name) => Some(name),
            ExpressionKind::Group(target) => target.variable(),
            _ => None,
        }
    }
}

pub(crate) struct Statement {
    pub(crate) location: Location,
    pub(crate) kind: StatementKind,
}

pub(crate) enum StatementKind {
    Block(Vec<Statement>),
    Declare(Binding, Option<Expression>),
    Assign(Expression, Option<&'static str>, Expression),
    Expression(Expression),
    If(Expression, Box<Statement>, Option<Box<Statement>>),
    Loop {
        initializer: Option<Box<Statement>>,
        condition: Expression,
        step: Option<Box<Statement>>,
        body: Box<Statement>,
        invariants: Vec<Contract>,
    },
    Contracts(Vec<Contract>),
    Guard(Vec<Contract>, Box<Statement>),
    Return(Option<Expression>),
    Assert(Expression),
    Error(Expression),
}
