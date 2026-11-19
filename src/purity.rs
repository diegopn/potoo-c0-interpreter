use std::collections::{HashMap, HashSet};

use crate::{
    Diagnostic, Location,
    check::Checked,
    syntax::{Expression, ExpressionKind, Statement, StatementKind},
};

#[derive(Default)]
struct Effects<'a> {
    writes_existing_memory: bool,
    calls: HashSet<&'a str>,
}

pub(crate) fn validate(
    program: &Checked<'_>,
    annotation_calls: &[(&str, Location)],
) -> Result<(), Diagnostic> {
    if annotation_calls.is_empty() {
        return Ok(());
    }
    let mut effects = HashMap::new();
    for (&name, target) in &program.functions {
        let mut effect = Effects::default();
        if let Some(definition) = target.definition {
            effect.block(
                definition.body.as_deref().expect("definition"),
                &mut HashSet::new(),
            );
        } else if matches!(
            name,
            "args_flag"
                | "args_int"
                | "args_string"
                | "args_parse"
                | "rand"
                | "file_close"
                | "file_readline"
                | "image_destroy"
        ) {
            effect.writes_existing_memory = true;
        }
        effects.insert(name, effect);
    }

    let mut impure: HashSet<_> = effects
        .iter()
        .filter_map(|(&name, effect)| effect.writes_existing_memory.then_some(name))
        .collect();
    loop {
        let count = impure.len();
        for (&name, effect) in &effects {
            if effect.calls.iter().any(|called| impure.contains(called)) {
                impure.insert(name);
            }
        }
        if impure.len() == count {
            break;
        }
    }
    if let Some((name, location)) = annotation_calls
        .iter()
        .find(|(name, _)| impure.contains(name))
    {
        return Err(location.error(format!(
            "'{name}' may modify previously allocated memory in a contract"
        )));
    }
    Ok(())
}

type Fresh<'a> = HashSet<&'a str>;

fn fresh_value(expression: &Expression, fresh: &Fresh<'_>) -> bool {
    match &expression.kind {
        ExpressionKind::Allocate(..) => true,
        ExpressionKind::Literal(crate::syntax::Literal::Null) => true,
        ExpressionKind::Group(inner) => fresh_value(inner, fresh),
        ExpressionKind::Variable(name) => fresh.contains(name.as_str()),
        ExpressionKind::Conditional(_, yes, no) => {
            fresh_value(yes, fresh) && fresh_value(no, fresh)
        }
        _ => false,
    }
}

fn fresh_memory(target: &Expression, fresh: &Fresh<'_>) -> bool {
    match &target.kind {
        ExpressionKind::Index(base, _) | ExpressionKind::Unary("*", base) => {
            fresh_value(base, fresh)
        }
        ExpressionKind::Field(base, _) | ExpressionKind::Group(base) => fresh_memory(base, fresh),
        _ => false,
    }
}

impl<'a> Effects<'a> {
    fn expression(&mut self, expression: &'a Expression) {
        match &expression.kind {
            ExpressionKind::Call(name, arguments) => {
                self.calls.insert(name);
                for argument in arguments {
                    self.expression(argument);
                }
            }
            ExpressionKind::Group(operand)
            | ExpressionKind::Unary(_, operand)
            | ExpressionKind::Field(operand, _)
            | ExpressionKind::Length(operand)
            | ExpressionKind::Allocate(_, Some(operand)) => self.expression(operand),
            ExpressionKind::Index(base, index) | ExpressionKind::Binary(_, base, index) => {
                self.expression(base);
                self.expression(index);
            }
            ExpressionKind::Conditional(condition, yes, no) => {
                self.expression(condition);
                self.expression(yes);
                self.expression(no);
            }
            _ => {}
        }
    }

    fn block(&mut self, statements: &'a [Statement], fresh: &mut Fresh<'a>) -> bool {
        let mut declarations = Vec::new();
        let mut continues = true;
        for statement in statements {
            let mut declaration = statement;
            while let StatementKind::Guard(_, guarded) = &declaration.kind {
                declaration = guarded;
            }
            if let StatementKind::Declare(binding, _) = &declaration.kind {
                declarations.push(binding.name.as_str());
            }
            if !self.statement(statement, fresh) {
                continues = false;
                break;
            }
        }
        for name in declarations {
            fresh.remove(name);
        }
        continues
    }

    fn statement(&mut self, statement: &'a Statement, fresh: &mut Fresh<'a>) -> bool {
        match &statement.kind {
            StatementKind::Block(statements) => return self.block(statements, fresh),
            StatementKind::Declare(binding, Some(value)) => {
                self.expression(value);
                if fresh_value(value, fresh) {
                    fresh.insert(&binding.name);
                }
            }
            StatementKind::Assign(target, operator, value) => {
                self.expression(target);
                self.expression(value);
                if let Some(name) = target.variable() {
                    if operator.is_none() && fresh_value(value, fresh) {
                        fresh.insert(name);
                    } else {
                        fresh.remove(name);
                    }
                } else {
                    self.writes_existing_memory |= !fresh_memory(target, fresh);
                }
            }
            StatementKind::If(condition, yes, no) => {
                self.expression(condition);
                let mut yes_fresh = fresh.clone();
                let yes_continues = self.statement(yes, &mut yes_fresh);
                let no_continues = no.as_deref().is_none_or(|no| self.statement(no, fresh));
                if !no_continues {
                    *fresh = yes_fresh;
                } else if yes_continues {
                    fresh.retain(|name| yes_fresh.contains(name));
                }
                return yes_continues || no_continues;
            }
            StatementKind::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                if let Some(initializer) = initializer {
                    self.statement(initializer, fresh);
                }
                self.expression(condition);
                loop {
                    let mut next = fresh.clone();
                    if self.statement(body, &mut next)
                        && let Some(step) = step
                    {
                        self.statement(step, &mut next);
                    }
                    let count = fresh.len();
                    fresh.retain(|name| next.contains(name));
                    if count == fresh.len() {
                        break;
                    }
                }
                if let Some(initializer) = initializer
                    && let StatementKind::Declare(binding, _) = &initializer.kind
                {
                    fresh.remove(binding.name.as_str());
                }
            }
            StatementKind::Guard(_, statement) => return self.statement(statement, fresh),
            StatementKind::Expression(expression) | StatementKind::Assert(expression) => {
                self.expression(expression)
            }
            StatementKind::Return(expression) => {
                if let Some(expression) = expression {
                    self.expression(expression);
                }
                return false;
            }
            StatementKind::Error(message) => {
                self.expression(message);
                return false;
            }
            StatementKind::Declare(_, None) | StatementKind::Contracts(_) => {}
        }
        true
    }
}
