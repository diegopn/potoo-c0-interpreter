use std::{collections::HashMap, rc::Rc, sync::Arc};

use crate::{
    CallSite, DebugEvent, DebugEventKind, Diagnostic, DiagnosticKind, Location, RunOptions,
    RuntimeHost,
    check::{CallTarget, Checked},
    heap::{Heap, Memory, Value},
    libraries::Resources,
    syntax::{
        Contract, ContractKind, Expression, ExpressionKind, Function, Statement, StatementKind,
        Type,
    },
};

type Scope = HashMap<String, Option<Value>>;

struct Frame {
    scopes: Vec<Scope>,
    result: Option<Value>,
}

impl Frame {
    fn parameters(function: &Function, args: &[Value]) -> Self {
        let parameters = function
            .parameters
            .iter()
            .zip(args)
            .map(|(parameter, value)| (parameter.name.clone(), Some(value.clone())))
            .collect();
        Self {
            scopes: vec![parameters],
            result: None,
        }
    }

    fn get(&self, name: &str) -> &Option<Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .expect("validated variable")
    }

    fn set(&mut self, name: &str, value: Value) {
        let variable = self
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
            .expect("validated variable");
        *variable = Some(value);
    }
}

pub(crate) fn run(
    program: &Checked<'_>,
    sources: &[Arc<str>],
    host: &mut impl RuntimeHost,
    options: RunOptions,
) -> Result<i32, Diagnostic> {
    let mut evaluator = Evaluator {
        program,
        sources,
        host,
        options,
        resources: Resources::default(),
        heap: Heap::default(),
        stack: Vec::new(),
    };
    let entry = program.functions["main"]
        .definition
        .expect("main definition");
    match evaluator.call("main", Vec::new(), entry.location)? {
        Value::Int(result) => Ok(result),
        _ => unreachable!("validation requires int main()"),
    }
}

struct Evaluator<'a, 'host, H> {
    program: &'a Checked<'a>,
    sources: &'a [Arc<str>],
    host: &'host mut H,
    options: RunOptions,
    resources: Resources,
    heap: Heap,
    stack: Vec<(&'a str, Location)>,
}

enum Place<'a> {
    Local(&'a str),
    Slot(Memory, usize),
    Invalid(Diagnostic),
}

impl Place<'_> {
    fn read(&self, frame: &Frame) -> Result<Value, Diagnostic> {
        match self {
            Self::Local(name) => Ok(frame
                .get(name)
                .as_ref()
                .expect("initialized variable")
                .clone()),
            Self::Slot(memory, index) => Ok(memory.borrow()[*index].clone()),
            Self::Invalid(error) => Err(error.location.error(error.message.clone())),
        }
    }

    fn write(self, frame: &mut Frame, value: Value) -> Result<(), Diagnostic> {
        match self {
            Self::Local(name) => frame.set(name, value),
            Self::Slot(memory, index) => memory.borrow_mut()[index] = value,
            Self::Invalid(error) => return Err(error),
        }
        Ok(())
    }
}
fn unary(operator: &str, value: Value) -> Value {
    match (operator, value) {
        ("!", Value::Bool(value)) => Value::Bool(!value),
        ("~", Value::Int(value)) => Value::Int(!value),
        ("-", Value::Int(value)) => Value::Int(value.wrapping_neg()),
        _ => unreachable!("validated unary operator"),
    }
}

fn comparison<T: Ord>(operator: &str, left: T, right: T) -> Option<Value> {
    let result = match operator {
        "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => return None,
    };
    Some(Value::Bool(result))
}

fn binary(
    operator: &str,
    left: Value,
    right: Value,
    location: Location,
) -> Result<Value, Diagnostic> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            if let Some(result) = comparison(operator, left, right) {
                return Ok(result);
            }
            let result = match operator {
                "+" => left.wrapping_add(right),
                "-" => left.wrapping_sub(right),
                "*" => left.wrapping_mul(right),
                "/" | "%" if right == 0 => {
                    return Err(location.error("division or remainder by zero"));
                }
                "/" => left
                    .checked_div(right)
                    .ok_or_else(|| location.error("integer division overflow"))?,
                "%" => left
                    .checked_rem(right)
                    .ok_or_else(|| location.error("integer remainder overflow"))?,
                "&" => left & right,
                "|" => left | right,
                "^" => left ^ right,
                "<<" | ">>" if !(0..32).contains(&right) => {
                    return Err(location.error("shift count must be between 0 and 31"));
                }
                "<<" => left.wrapping_shl(right as u32),
                ">>" => left >> right,
                _ => unreachable!("validated integer operator"),
            };
            Ok(Value::Int(result))
        }
        (Value::Char(left), Value::Char(right)) => {
            Ok(comparison(operator, left, right).expect("validated character operator"))
        }
        (Value::Bool(left), Value::Bool(right)) => Ok(match operator {
            "&&" | "||" => Value::Bool(right),
            _ => comparison(operator, left, right).expect("validated boolean operator"),
        }),
        (Value::Pointer(left), Value::Pointer(right))
        | (Value::Array(left), Value::Array(right)) => {
            let equal = match (left, right) {
                (Some(left), Some(right)) => Rc::ptr_eq(&left, &right),
                (None, None) => true,
                _ => false,
            };
            Ok(Value::Bool(if operator == "==" { equal } else { !equal }))
        }
        _ => unreachable!("validated binary operands"),
    }
}

fn boolean(value: Value) -> bool {
    let Value::Bool(value) = value else {
        unreachable!("validated boolean expression")
    };
    value
}

impl<'a, H: RuntimeHost> Evaluator<'a, '_, H> {
    fn event(&mut self, kind: DebugEventKind, location: Location) -> Result<(), Diagnostic> {
        let function = self.stack.last().expect("active call").0;
        let event = DebugEvent {
            kind,
            function,
            location,
            source: self.sources.get(location.source_id).map(AsRef::as_ref),
            depth: self.stack.len(),
        };
        if self.host.event(event).is_break() {
            return Err(location
                .error("execution cancelled")
                .with_kind(DiagnosticKind::Cancelled));
        }
        Ok(())
    }

    fn call(
        &mut self,
        name: &str,
        args: Vec<Value>,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let (&name, target) = self
            .program
            .functions
            .get_key_value(name)
            .expect("validated function");
        // Evaluator frames follow the host stack.
        const MAX_CALL_DEPTH: usize = 128;
        if self.stack.len() == MAX_CALL_DEPTH {
            return Err(location.error("function call depth limit exceeded"));
        }
        self.stack.push((name, location));
        let result = self
            .call_body(name, target, args, location)
            .map_err(|mut error| {
                if error.call_stack.is_empty() {
                    error.call_stack = self
                        .stack
                        .iter()
                        .rev()
                        .map(|(name, location)| CallSite {
                            function: (*name).to_owned(),
                            source: self.sources.get(location.source_id).cloned(),
                            location: *location,
                        })
                        .collect();
                    if error.kind == DiagnosticKind::Validation {
                        error.kind = DiagnosticKind::Runtime;
                    }
                }
                error
            });
        self.stack.pop();
        result
    }

    fn call_body(
        &mut self,
        name: &str,
        target: &CallTarget<'_>,
        args: Vec<Value>,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let definition = target.definition;
        let anchor = definition.map_or(location, |function| function.location);
        self.event(DebugEventKind::EnterFunction, anchor)?;
        if self.options.check_contracts {
            self.function_contracts(target, &args, None, ContractKind::Requires)?;
        }
        let value = if let Some(function) = definition {
            let mut frame = Frame::parameters(function, &args);
            self.block(function.body.as_deref().expect("definition"), &mut frame)?
                .unwrap_or(Value::Void)
        } else {
            self.resources
                .call(name, &args, self.host, &mut self.heap, location)?
        };
        if self.options.check_contracts {
            self.function_contracts(target, &args, Some(&value), ContractKind::Ensures)?;
        }
        self.event(DebugEventKind::LeaveFunction, anchor)?;
        Ok(value)
    }

    fn function_contracts(
        &mut self,
        target: &CallTarget<'_>,
        args: &[Value],
        result: Option<&Value>,
        kind: ContractKind,
    ) -> Result<(), Diagnostic> {
        for declaration in &target.annotations {
            let mut frame = Frame::parameters(declaration, args);
            frame.result = result.cloned();
            for contract in declaration
                .contracts
                .iter()
                .filter(|contract| contract.kind == kind)
            {
                self.contract(contract, &frame)?;
            }
        }
        Ok(())
    }

    fn contract(&mut self, contract: &Contract, frame: &Frame) -> Result<(), Diagnostic> {
        if self.options.check_contracts && !boolean(self.expression(&contract.expression, frame)?) {
            let description = match contract.kind {
                ContractKind::Requires => "precondition",
                ContractKind::Ensures => "postcondition",
                ContractKind::Invariant => "loop invariant",
                ContractKind::Assert => "assertion annotation",
            };
            return Err(contract
                .expression
                .location
                .error(format!("{description} failed")));
        }
        Ok(())
    }

    fn default_value(&mut self, ty: &Type) -> Value {
        match ty {
            Type::Int => Value::Int(0),
            Type::Bool => Value::Bool(false),
            Type::Char => Value::Char(0),
            Type::String => Value::string([]),
            Type::Pointer(_) => Value::Pointer(None),
            Type::Array(_) => Value::Array(None),
            Type::Struct(name) => {
                let fields = self.program.structures[name.as_str()]
                    .iter()
                    .map(|binding| self.default_value(&binding.ty))
                    .collect();
                Value::Struct(name.clone(), self.heap.block(fields))
            }
            Type::Null | Type::Void => unreachable!("validated allocation type"),
        }
    }

    fn place<'e>(
        &mut self,
        expression: &'e Expression,
        frame: &Frame,
    ) -> Result<Place<'e>, Diagnostic> {
        let location = expression.location;
        Ok(match &expression.kind {
            ExpressionKind::Variable(name) => Place::Local(name),
            ExpressionKind::Group(inner) => return self.place(inner, frame),
            ExpressionKind::Unary("*", value) => match self.expression(value, frame)? {
                Value::Pointer(Some(memory)) => Place::Slot(memory, 0),
                Value::Pointer(None) => Place::Invalid(location.error("null pointer dereference")),
                _ => unreachable!("validated dereference"),
            },
            ExpressionKind::Field(base, field) => {
                let base = self.place(base, frame)?;
                if matches!(base, Place::Invalid(_)) {
                    return Ok(base);
                }
                let Value::Struct(name, memory) = base.read(frame)? else {
                    unreachable!("validated field receiver")
                };
                let index = self.program.structures[name.as_str()]
                    .iter()
                    .position(|binding| binding.name == *field)
                    .expect("validated field name");
                Place::Slot(memory, index)
            }
            ExpressionKind::Index(base, index) => {
                let Value::Array(memory) = self.expression(base, frame)? else {
                    unreachable!("validated array receiver")
                };
                let Value::Int(index) = self.expression(index, frame)? else {
                    unreachable!("validated array index")
                };
                let length = memory.as_ref().map_or(0, |values| values.borrow().len());
                if index >= 0 && (index as usize) < length {
                    Place::Slot(memory.expect("nonempty array"), index as usize)
                } else {
                    Place::Invalid(
                        location.error(format!("array index {index} is outside length {length}")),
                    )
                }
            }
            _ => unreachable!("validated memory expression"),
        })
    }

    fn expression(&mut self, expression: &Expression, frame: &Frame) -> Result<Value, Diagnostic> {
        let location = expression.location;
        match &expression.kind {
            ExpressionKind::Literal(literal) => Ok(literal.into()),
            ExpressionKind::Group(inner) => self.expression(inner, frame),
            ExpressionKind::Variable(_)
            | ExpressionKind::Unary("*", _)
            | ExpressionKind::Index(..)
            | ExpressionKind::Field(..) => self.place(expression, frame)?.read(frame),
            ExpressionKind::Call(name, expressions) => {
                let args = expressions
                    .iter()
                    .map(|value| self.expression(value, frame))
                    .collect::<Result<_, _>>()?;
                self.call(name, args, location)
            }
            ExpressionKind::Unary(operator, operand) => {
                Ok(unary(operator, self.expression(operand, frame)?))
            }
            ExpressionKind::Binary(operator, left, right) => {
                let left = self.expression(left, frame)?;
                if matches!(
                    (*operator, &left),
                    ("&&", Value::Bool(false)) | ("||", Value::Bool(true))
                ) {
                    return Ok(left);
                }
                let right = self.expression(right, frame)?;
                binary(operator, left, right, location)
            }
            ExpressionKind::Conditional(condition, yes, no) => {
                let branch = if boolean(self.expression(condition, frame)?) {
                    yes
                } else {
                    no
                };
                self.expression(branch, frame)
            }
            ExpressionKind::Allocate(ty, None) => {
                let value = self.default_value(ty);
                Ok(Value::Pointer(Some(self.heap.block(vec![value]))))
            }
            ExpressionKind::Allocate(ty, Some(length)) => {
                let Value::Int(length) = self.expression(length, frame)? else {
                    unreachable!("validated array length")
                };
                let length =
                    usize::try_from(length).map_err(|_| location.error("negative array length"))?;
                let mut elements = Vec::new();
                elements
                    .try_reserve_exact(length)
                    .map_err(|_| location.error("could not allocate array"))?;
                elements.resize_with(length, || self.default_value(ty));
                Ok(Value::Array(Some(self.heap.block(elements))))
            }
            ExpressionKind::Length(array) => {
                let Value::Array(memory) = self.expression(array, frame)? else {
                    unreachable!("validated length operand")
                };
                Ok(Value::Int(
                    memory.map_or(0, |array| array.borrow().len() as i32),
                ))
            }
            ExpressionKind::Result => Ok(frame.result.clone().expect("validated result binding")),
        }
    }

    fn block(
        &mut self,
        statements: &[Statement],
        frame: &mut Frame,
    ) -> Result<Option<Value>, Diagnostic> {
        for statement in statements {
            if let Some(value) = self.statement(statement, frame)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn statement(
        &mut self,
        statement: &Statement,
        frame: &mut Frame,
    ) -> Result<Option<Value>, Diagnostic> {
        let location = statement.location;
        self.event(DebugEventKind::Statement, location)?;
        match &statement.kind {
            StatementKind::Block(statements) => {
                frame.scopes.push(HashMap::new());
                let result = self.block(statements, frame);
                frame.scopes.pop();
                return result;
            }
            StatementKind::Declare(binding, initial) => {
                let value = initial
                    .as_ref()
                    .map(|expression| self.expression(expression, frame))
                    .transpose()?;
                frame
                    .scopes
                    .last_mut()
                    .expect("active scope")
                    .insert(binding.name.clone(), value);
            }
            StatementKind::Assign(target, operator, expression) => {
                let destination = self.place(target, frame)?;
                let previous = operator.map(|_| destination.read(frame)).transpose()?;
                let mut value = self.expression(expression, frame)?;
                if let Some((operator, previous)) = operator.zip(previous) {
                    value = binary(operator, previous, value, location)?;
                }
                destination.write(frame, value)?;
            }
            StatementKind::Expression(expression) => {
                self.expression(expression, frame)?;
            }
            StatementKind::If(condition, yes, no) => {
                if boolean(self.expression(condition, frame)?) {
                    return self.statement(yes, frame);
                }
                if let Some(no) = no {
                    return self.statement(no, frame);
                }
            }
            StatementKind::Loop {
                initializer,
                condition,
                step,
                body,
                invariants,
            } => {
                frame.scopes.push(HashMap::new());
                let result = self.loop_body(
                    initializer.as_deref(),
                    condition,
                    step.as_deref(),
                    body,
                    invariants,
                    frame,
                );
                frame.scopes.pop();
                return result;
            }
            StatementKind::Guard(contracts, statement) => {
                for contract in contracts {
                    self.contract(contract, frame)?;
                }
                return self.statement(statement, frame);
            }
            StatementKind::Contracts(contracts) => {
                for contract in contracts {
                    self.contract(contract, frame)?;
                }
            }
            StatementKind::Return(expression) => {
                let value = expression
                    .as_ref()
                    .map(|value| self.expression(value, frame))
                    .transpose()?
                    .unwrap_or(Value::Void);
                return Ok(Some(value));
            }
            StatementKind::Assert(condition) => {
                if !boolean(self.expression(condition, frame)?) {
                    return Err(location.error("assertion failed"));
                }
            }
            StatementKind::Error(message) => {
                let Value::String(message) = self.expression(message, frame)? else {
                    unreachable!("validated error argument")
                };
                return Err(location
                    .error(String::from_utf8_lossy(&message))
                    .with_kind(DiagnosticKind::ProgramError));
            }
        }
        Ok(None)
    }

    fn loop_body(
        &mut self,
        initializer: Option<&Statement>,
        condition: &Expression,
        step: Option<&Statement>,
        body: &Statement,
        invariants: &[Contract],
        frame: &mut Frame,
    ) -> Result<Option<Value>, Diagnostic> {
        if let Some(initializer) = initializer {
            self.statement(initializer, frame)?;
        }
        loop {
            self.event(DebugEventKind::LoopCondition, condition.location)?;
            for invariant in invariants
                .iter()
                .filter(|contract| contract.kind == ContractKind::Invariant)
            {
                self.contract(invariant, frame)?;
            }
            if !boolean(self.expression(condition, frame)?) {
                return Ok(None);
            }
            for assertion in invariants
                .iter()
                .filter(|contract| contract.kind == ContractKind::Assert)
            {
                self.contract(assertion, frame)?;
            }
            if let Some(value) = self.statement(body, frame)? {
                return Ok(Some(value));
            }
            if let Some(step) = step {
                self.statement(step, frame)?;
            }
        }
    }
}
