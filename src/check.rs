use std::collections::{HashMap, HashSet, hash_map::Entry};

use crate::{
    CheckOptions, Diagnostic, Location, SourceOrigin, SourceOrigins, libraries,
    syntax::{
        Binding, Contract, ContractKind, Declaration, Expression, ExpressionKind, Function,
        Statement, StatementKind, Type,
    },
};

#[derive(Default)]
pub(crate) struct CallTarget<'a> {
    pub(crate) definition: Option<&'a Function>,
    pub(crate) annotations: Vec<&'a Function>,
    pub(crate) native: bool,
}

impl CallTarget<'_> {
    fn defined(&self) -> bool {
        self.native || self.definition.is_some()
    }
}

pub(crate) struct Checked<'a> {
    pub(crate) functions: HashMap<&'a str, CallTarget<'a>>,
    pub(crate) structures: HashMap<&'a str, &'a [Binding]>,
}

struct Signature<'a> {
    result: Type,
    parameters: Vec<Type>,
    target: CallTarget<'a>,
}

#[derive(Clone, Copy)]
struct Variable<'a> {
    ty: &'a Type,
    initialized: bool,
}

type Variables<'a> = HashMap<&'a str, Variable<'a>>;

pub(crate) fn validate<'a>(
    declarations: &'a [Declaration],
    origins: &'a [SourceOrigins],
    options: CheckOptions,
) -> Result<Checked<'a>, Diagnostic> {
    let mut checker = Checker {
        signatures: HashMap::new(),
        structures: HashMap::new(),
        origins,
        opaque: HashMap::new(),
        format_forms: HashSet::new(),
        aliases: HashSet::new(),
        referenced: Vec::new(),
        annotation_calls: Vec::new(),
        annotation: None,
        return_type: &Type::Int,
        protected: HashSet::new(),
    };

    checker.signatures.insert(
        "main",
        Signature {
            result: Type::Int,
            parameters: Vec::new(),
            target: CallTarget::default(),
        },
    );
    let mut ensured_parameters = HashMap::<&str, HashSet<usize>>::new();
    for declaration in declarations {
        match declaration {
            Declaration::Struct {
                location,
                name,
                fields: None,
            } => {
                for origin in origins[location.source_id].declaring_roles() {
                    let owner = match origin {
                        SourceOrigin::Builtin => None,
                        SourceOrigin::LibraryHeader(library) => Some(library.as_ref()),
                        _ => continue,
                    };
                    checker.opaque.entry(name).or_default().insert(owner);
                }
            }
            Declaration::Function(function) => {
                if function.native && libraries::is_formatted(&function.name) {
                    checker.format_forms.insert(&function.name);
                }
                for contract in function
                    .contracts
                    .iter()
                    .filter(|c| c.kind == ContractKind::Ensures)
                {
                    contract.expression.visit(&mut |expression| {
                        if let ExpressionKind::Variable(name) = &expression.kind
                            && let Some(index) =
                                function.parameters.iter().position(|p| p.name == *name)
                        {
                            ensured_parameters
                                .entry(&function.name)
                                .or_default()
                                .insert(index);
                        }
                    });
                }
            }
            _ => {}
        }
    }

    for declaration in declarations {
        match declaration {
            Declaration::Function(function) => {
                checker.function(function, ensured_parameters.get(function.name.as_str()))?;
            }
            Declaration::Struct {
                location,
                name,
                fields: Some(fields),
            } => {
                checker.structure(*location, name, fields)?;
            }
            Declaration::Struct { fields: None, .. } => {}
            Declaration::Alias(alias) => {
                well_formed_type(&alias.ty, false, alias.location)?;
                if checker.signatures.contains_key(alias.name.as_str()) {
                    return Err(alias
                        .location
                        .error(format!("type '{}' conflicts with a function", alias.name)));
                }
                checker.aliases.insert(&alias.name);
            }
        }
    }

    if !checker.signatures["main"].target.defined() {
        return Err(Location::start(0).error("function 'main' has no definition"));
    }
    for (name, location) in checker.referenced {
        if !checker.signatures[name].target.defined() {
            return Err(location.error(format!("function '{name}' has no definition")));
        }
    }
    let checked = Checked {
        structures: checker
            .structures
            .into_iter()
            .map(|(name, layout)| (name, layout.fields))
            .collect(),
        functions: checker
            .signatures
            .into_iter()
            .filter_map(|(name, signature)| {
                signature
                    .target
                    .defined()
                    .then_some((name, signature.target))
            })
            .collect(),
    };
    if options.check_purity {
        super::purity::validate(&checked, &checker.annotation_calls)?;
    }
    Ok(checked)
}

struct Checker<'a> {
    signatures: HashMap<&'a str, Signature<'a>>,
    structures: HashMap<&'a str, StructLayout<'a>>,
    origins: &'a [SourceOrigins],
    opaque: HashMap<&'a str, HashSet<Option<&'a str>>>,
    format_forms: HashSet<&'a str>,
    aliases: HashSet<&'a str>,
    referenced: Vec<(&'a str, Location)>,
    annotation_calls: Vec<(&'a str, Location)>,
    annotation: Option<ContractKind>,
    return_type: &'a Type,
    protected: HashSet<&'a str>,
}

struct StructLayout<'a> {
    fields: &'a [Binding],
    source_id: usize,
    private_to: Option<&'a str>,
}

fn require(actual: Type, expected: &Type, location: Location) -> Result<(), Diagnostic> {
    if expected.accepts(&actual) {
        Ok(())
    } else {
        Err(location.error(format!("expected {expected}, found {actual}")))
    }
}

fn well_formed_type(ty: &Type, return_type: bool, location: Location) -> Result<(), Diagnostic> {
    match ty {
        Type::Void if !return_type => {
            Err(location.error("only a function's return type may be void"))
        }
        Type::Pointer(ty) | Type::Array(ty) => well_formed_type(ty, false, location),
        _ => Ok(()),
    }
}

fn small(ty: &Type, location: Location) -> Result<(), Diagnostic> {
    if ty.is_small() {
        Ok(())
    } else {
        Err(location.error(format!("expected a small type, found {ty}")))
    }
}

fn read(variable: Variable<'_>, name: &str, location: Location) -> Result<Type, Diagnostic> {
    if variable.initialized {
        Ok(variable.ty.clone())
    } else {
        Err(location.error(format!("variable '{name}' may be uninitialized")))
    }
}

fn binary_type(
    operator: &str,
    left: Type,
    right: Type,
    location: Location,
) -> Result<Type, Diagnostic> {
    let ty = if right.accepts(&left) {
        right
    } else {
        require(right, &left, location)?;
        left
    };
    let result = match (operator, &ty) {
        (
            "==" | "!=",
            Type::Int | Type::Bool | Type::Char | Type::Pointer(_) | Type::Array(_) | Type::Null,
        )
        | ("<" | "<=" | ">" | ">=", Type::Int | Type::Char)
        | ("&&" | "||", Type::Bool) => Type::Bool,
        ("+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<<" | ">>", Type::Int) => Type::Int,
        _ => {
            return Err(location.error(format!(
                "operator '{operator}' does not accept {ty} operands"
            )));
        }
    };
    Ok(result)
}

impl<'a> Checker<'a> {
    fn declare(
        &self,
        variables: &mut Variables<'a>,
        binding: &'a Binding,
        initialized: bool,
    ) -> Result<(), Diagnostic> {
        well_formed_type(&binding.ty, false, binding.location)?;
        small(&binding.ty, binding.location)?;
        if self.aliases.contains(binding.name.as_str()) {
            return Err(binding
                .location
                .error(format!("variable '{}' conflicts with a type", binding.name)));
        }
        if libraries::is_formatted(&binding.name)
            && self
                .signatures
                .get(binding.name.as_str())
                .is_some_and(|signature| signature.target.native)
        {
            return Err(binding.location.error(format!(
                "'{}' is a reserved library format form",
                binding.name
            )));
        }
        match variables.entry(&binding.name) {
            Entry::Occupied(_) => Err(binding
                .location
                .error(format!("variable '{}' is already in scope", binding.name))),
            Entry::Vacant(entry) => {
                entry.insert(Variable {
                    ty: &binding.ty,
                    initialized,
                });
                Ok(())
            }
        }
    }

    fn layout(&self, name: &str, location: Location) -> Result<&'a [Binding], Diagnostic> {
        self.structures
            .get(name)
            .filter(|layout| {
                layout.private_to.is_none_or(|owner| {
                    layout.source_id == location.source_id
                        || self.origins[location.source_id].belongs_to(owner)
                })
            })
            .map(|layout| layout.fields)
            .ok_or_else(|| location.error(format!("struct '{name}' has no visible definition")))
    }

    fn require_layout(&self, ty: &Type, location: Location) -> Result<(), Diagnostic> {
        if let Type::Struct(name) = ty {
            self.layout(name, location)?;
        }
        Ok(())
    }

    fn private_struct_owner(
        &self,
        name: &str,
        location: Location,
    ) -> Result<Option<&'a str>, Diagnostic> {
        let origins = &self.origins[location.source_id];
        let reserved = self.opaque.get(name);
        let opaque_error =
            || location.error(format!("struct '{name}' is opaque outside its library"));
        if reserved.is_some_and(|owners| owners.contains(&None)) {
            return Err(opaque_error());
        }
        if origins.routes.contains(&SourceOrigin::Builtin) {
            return Ok(None);
        }

        let implementation = |origin: &'a SourceOrigin| match origin {
            SourceOrigin::LibraryImplementation(library) => Some(library.as_ref()),
            _ => None,
        };
        let is_owner = |library| reserved.is_none_or(|owners| owners.contains(&Some(library)));
        let entries = origins.entries.iter().filter_map(implementation);
        let routes = origins.routes.iter().filter_map(implementation);
        let claimed = routes.clone().filter(|&library| is_owner(library));
        let has_header_entry = origins
            .entries
            .iter()
            .any(|origin| matches!(origin, SourceOrigin::LibraryHeader(_)));
        let has_implementation_entry = entries.clone().next().is_some();
        let has_implementation_route = routes.clone().next().is_some();
        let needs_implementation_owner = has_implementation_entry
            || (has_implementation_route
                && (!has_header_entry || (reserved.is_some() && claimed.clone().next().is_some())));

        if needs_implementation_owner {
            let mut candidates = if has_implementation_entry {
                entries
            } else {
                routes
            }
            .filter(|&library| is_owner(library));
            let owner = candidates.next().ok_or_else(opaque_error)?;
            if candidates.next().is_some() {
                return Err(location.error(format!(
                    "struct '{name}' has multiple possible library owners"
                )));
            }
            // Publishing an implementation layout takes a header route from its owner.
            let public = origins.routes.iter().any(|origin| {
                matches!(origin, SourceOrigin::LibraryHeader(library) if library.as_ref() == owner)
            });
            return Ok((!public).then_some(owner));
        }
        if reserved.is_some_and(|owners| {
            !origins.declaring_roles().iter().any(|origin| {
                matches!(origin, SourceOrigin::LibraryHeader(library) if owners.contains(&Some(library.as_ref())))
            })
        }) {
            return Err(opaque_error());
        }
        Ok(None)
    }

    fn structure(
        &mut self,
        location: Location,
        name: &'a str,
        fields: &'a [Binding],
    ) -> Result<(), Diagnostic> {
        let private_to = self.private_struct_owner(name, location)?;
        if self.structures.contains_key(name) {
            return Err(location.error(format!("struct '{name}' is already defined")));
        }
        let mut names = HashSet::new();
        for field in fields {
            well_formed_type(&field.ty, false, field.location)?;
            self.require_layout(&field.ty, field.location)?;
            if !names.insert(&field.name) {
                return Err(field
                    .location
                    .error(format!("field '{}' is declared twice", field.name)));
            }
        }
        self.structures.insert(
            name,
            StructLayout {
                fields,
                source_id: location.source_id,
                private_to,
            },
        );
        Ok(())
    }

    fn function(
        &mut self,
        function: &'a Function,
        ensured_parameters: Option<&HashSet<usize>>,
    ) -> Result<(), Diagnostic> {
        let mut variables = HashMap::new();
        for parameter in &function.parameters {
            self.declare(&mut variables, parameter, true)?;
        }
        if self.aliases.contains(function.name.as_str()) {
            return Err(function.location.error(format!(
                "function '{}' conflicts with a type",
                function.name
            )));
        }
        well_formed_type(&function.result, true, function.location)?;
        if function.result != Type::Void {
            small(&function.result, function.location)?;
        }

        if !function.native && self.format_forms.contains(function.name.as_str()) {
            return Err(function.location.error(format!(
                "'{}' is a reserved library format form",
                function.name
            )));
        }
        let signature = self
            .signatures
            .entry(&function.name)
            .or_insert_with(|| Signature {
                result: function.result.clone(),
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.ty.clone())
                    .collect(),
                target: CallTarget::default(),
            });
        if signature.result != function.result
            || !signature
                .parameters
                .iter()
                .eq(function.parameters.iter().map(|parameter| &parameter.ty))
        {
            return Err(function
                .location
                .error(format!("inconsistent declaration of '{}'", function.name)));
        }

        if function.body.is_some() || function.native {
            if signature.target.defined() {
                return Err(function
                    .location
                    .error(format!("function '{}' is already defined", function.name)));
            }
            if function.native {
                signature.target.native = true;
            } else {
                signature.target.definition = Some(function);
            }
        }
        if !function.contracts.is_empty() {
            signature.target.annotations.push(function);
        }

        self.return_type = &function.result;
        self.contracts(
            &function.contracts,
            &variables,
            &[ContractKind::Requires, ContractKind::Ensures],
        )?;
        if let Some(body) = &function.body {
            self.protected.clear();
            if let Some(indices) = ensured_parameters {
                self.protected.extend(
                    indices
                        .iter()
                        .filter_map(|&index| function.parameters.get(index))
                        .map(|parameter| parameter.name.as_str()),
                );
            }
            let falls_through = self.block(body, &mut variables)?;
            if falls_through && function.result != Type::Void {
                return Err(function.location.error(format!(
                    "function '{}' can reach its end without returning",
                    function.name
                )));
            }
        }
        Ok(())
    }

    fn contracts(
        &mut self,
        contracts: &'a [Contract],
        variables: &Variables<'a>,
        allowed: &[ContractKind],
    ) -> Result<(), Diagnostic> {
        for contract in contracts {
            if !allowed.contains(&contract.kind) {
                return Err(contract
                    .expression
                    .location
                    .error("contract annotation is not allowed here"));
            }
            self.annotation = Some(contract.kind);
            self.expect_expression(&contract.expression, &Type::Bool, variables)?;
        }
        self.annotation = None;
        Ok(())
    }

    fn call_expression(
        &mut self,
        name: &'a str,
        args: &'a [Expression],
        variables: &Variables<'a>,
        location: Location,
    ) -> Result<Type, Diagnostic> {
        let signature = self.signatures.get(name).ok_or_else(|| {
            location.error(format!("function '{name}' must be declared before use"))
        })?;
        let formatted = signature.target.native && libraries::is_formatted(name);
        // A local can hide an ordinary function. The format forms are special.
        if variables.contains_key(name) {
            return Err(location.error(format!("'{name}' refers to a local variable")));
        }
        let parameters = if formatted {
            libraries::format_types(args, location)?
        } else {
            signature.parameters.clone()
        };
        if parameters.len() != args.len() {
            return Err(location.error(format!(
                "function '{name}' expects {} argument(s), found {}",
                parameters.len(),
                args.len()
            )));
        }
        let result = signature.result.clone();
        for (argument, ty) in args.iter().zip(&parameters) {
            self.expect_expression(argument, ty, variables)?;
        }
        self.referenced.push((name, location));
        if self.annotation.is_some() {
            self.annotation_calls.push((name, location));
        }
        Ok(result)
    }

    fn expression(
        &mut self,
        expression: &'a Expression,
        variables: &Variables<'a>,
    ) -> Result<Type, Diagnostic> {
        let location = expression.location;
        match &expression.kind {
            ExpressionKind::Literal(value) => Ok(value.value_type()),
            ExpressionKind::Group(inner) => self.expression(inner, variables),
            ExpressionKind::Variable(name) => {
                let variable = variables
                    .get(name.as_str())
                    .ok_or_else(|| location.error(format!("unknown variable '{name}'")))?;
                read(*variable, name, location)
            }
            ExpressionKind::Call(name, args) => {
                self.call_expression(name, args, variables, location)
            }
            ExpressionKind::Unary("*", operand) => match self.expression(operand, variables)? {
                Type::Pointer(pointee) => Ok(*pointee),
                ty => Err(location.error(format!("cannot dereference {ty}"))),
            },
            ExpressionKind::Unary(op, operand) => {
                let ty = if *op == "!" { Type::Bool } else { Type::Int };
                self.expect_expression(operand, &ty, variables)?;
                Ok(ty)
            }
            ExpressionKind::Binary(op, left, right) => {
                let left = self.expression(left, variables)?;
                let right = self.expression(right, variables)?;
                binary_type(op, left, right, location)
            }
            ExpressionKind::Conditional(condition, yes, no) => {
                self.expect_expression(condition, &Type::Bool, variables)?;
                let yes_type = self.expression(yes, variables)?;
                let no_type = self.expression(no, variables)?;
                small(&yes_type, yes.location)?;
                if no_type.accepts(&yes_type) {
                    Ok(no_type)
                } else {
                    require(no_type, &yes_type, no.location)?;
                    Ok(yes_type)
                }
            }
            ExpressionKind::Field(base, field) => {
                let Type::Struct(name) = self.expression(base, variables)? else {
                    return Err(location.error("field access requires a struct"));
                };
                let fields = self.layout(&name, location)?;
                fields
                    .iter()
                    .find(|binding| &binding.name == field)
                    .map(|binding| binding.ty.clone())
                    .ok_or_else(|| {
                        location.error(format!("struct '{name}' has no field '{field}'"))
                    })
            }
            ExpressionKind::Index(base, index) => {
                let Type::Array(element) = self.expression(base, variables)? else {
                    return Err(location.error("indexing requires an array"));
                };
                self.expect_expression(index, &Type::Int, variables)?;
                self.require_layout(&element, location)?;
                Ok(*element)
            }
            ExpressionKind::Allocate(ty, size) => {
                well_formed_type(ty, false, location)?;
                self.require_layout(ty, location)?;
                Ok(if let Some(size) = size {
                    self.expect_expression(size, &Type::Int, variables)?;
                    Type::Array(Box::new(ty.clone()))
                } else {
                    Type::Pointer(Box::new(ty.clone()))
                })
            }
            ExpressionKind::Length(operand) => {
                if self.annotation.is_none() {
                    return Err(location.error("array length is available only in contracts"));
                }
                if !matches!(self.expression(operand, variables)?, Type::Array(_)) {
                    return Err(location.error("length requires an array"));
                }
                Ok(Type::Int)
            }
            ExpressionKind::Result => {
                if self.annotation != Some(ContractKind::Ensures) || self.return_type == &Type::Void
                {
                    return Err(location.error("result is available only in a postcondition of a function that returns a value"));
                }
                Ok(self.return_type.clone())
            }
        }
    }

    fn expect_expression(
        &mut self,
        expression: &'a Expression,
        expected: &Type,
        variables: &Variables<'a>,
    ) -> Result<(), Diagnostic> {
        require(
            self.expression(expression, variables)?,
            expected,
            expression.location,
        )
    }

    fn block(
        &mut self,
        statements: &'a [Statement],
        variables: &mut Variables<'a>,
    ) -> Result<bool, Diagnostic> {
        let mut falls_through = true;
        for statement in statements {
            if !falls_through {
                variables
                    .values_mut()
                    .for_each(|variable| variable.initialized = true);
            }
            falls_through &= self.statement(statement, variables)?;
        }
        Ok(falls_through)
    }

    fn statement(
        &mut self,
        statement: &'a Statement,
        variables: &mut Variables<'a>,
    ) -> Result<bool, Diagnostic> {
        let location = statement.location;
        match &statement.kind {
            StatementKind::Block(statements) => {
                let mut scoped = variables.clone();
                let falls_through = self.block(statements, &mut scoped)?;
                for (name, variable) in variables {
                    variable.initialized = scoped[name].initialized;
                }
                return Ok(falls_through);
            }
            StatementKind::Declare(binding, initial) => {
                if let Some(initial) = initial {
                    self.expect_expression(initial, &binding.ty, variables)?;
                }
                self.declare(variables, binding, initial.is_some())?;
            }
            StatementKind::Assign(target, operator, value) => {
                let local = target.variable();
                let ty = if let Some(name) = local {
                    if self.protected.contains(name) {
                        return Err(
                            location.error(format!("parameter '{name}' occurs in a postcondition"))
                        );
                    }
                    let variable = variables
                        .get(name)
                        .ok_or_else(|| location.error(format!("unknown variable '{name}'")))?;
                    if operator.is_some() {
                        read(*variable, name, location)?
                    } else {
                        variable.ty.clone()
                    }
                } else {
                    self.expression(target, variables)?
                };
                small(&ty, target.location)?;
                let right = self.expression(value, variables)?;
                if let Some(operator) = operator {
                    require(
                        binary_type(operator, ty.clone(), right, location)?,
                        &ty,
                        location,
                    )?;
                } else {
                    require(right, &ty, value.location)?;
                }
                if let Some(name) = local {
                    variables
                        .get_mut(name)
                        .expect("declared variable")
                        .initialized = true;
                }
            }
            StatementKind::Expression(expression) => {
                let ty = self.expression(expression, variables)?;
                if ty != Type::Void {
                    small(&ty, expression.location)?;
                }
            }
            StatementKind::If(condition, yes, no) => {
                self.expect_expression(condition, &Type::Bool, variables)?;
                let mut yes_variables = variables.clone();
                let mut no_variables = variables.clone();
                let yes_continues = self.statement(yes, &mut yes_variables)?;
                let no_continues = match no {
                    Some(no) => self.statement(no, &mut no_variables)?,
                    None => true,
                };
                for (name, variable) in variables {
                    variable.initialized = (!yes_continues || yes_variables[name].initialized)
                        && (!no_continues || no_variables[name].initialized);
                }
                return Ok(yes_continues || no_continues);
            }
            StatementKind::Loop {
                initializer,
                condition,
                step,
                body,
                invariants,
            } => {
                let mut scoped = variables.clone();
                if let Some(initializer) = initializer {
                    self.statement(initializer, &mut scoped)?;
                }
                self.expect_expression(condition, &Type::Bool, &scoped)?;
                for (name, variable) in variables {
                    variable.initialized = scoped[name].initialized;
                }
                self.contracts(
                    invariants,
                    &scoped,
                    &[ContractKind::Invariant, ContractKind::Assert],
                )?;
                let continues = self.statement(body, &mut scoped)?;
                if let Some(step) = step {
                    if !continues {
                        scoped
                            .values_mut()
                            .for_each(|variable| variable.initialized = true);
                    }
                    self.statement(step, &mut scoped)?;
                }
            }
            StatementKind::Contracts(contracts) => {
                self.contracts(contracts, variables, &[ContractKind::Assert])?
            }
            StatementKind::Guard(contracts, statement) => {
                self.contracts(contracts, variables, &[ContractKind::Assert])?;
                return self.statement(statement, variables);
            }
            StatementKind::Return(value) => {
                match value {
                    Some(value) if self.return_type != &Type::Void => {
                        self.expect_expression(value, self.return_type, variables)?;
                    }
                    Some(_) => return Err(location.error("a void function cannot return a value")),
                    None => require(Type::Void, self.return_type, location)?,
                }
                return Ok(false);
            }
            StatementKind::Assert(condition) => {
                self.expect_expression(condition, &Type::Bool, variables)?
            }
            StatementKind::Error(message) => {
                self.expect_expression(message, &Type::String, variables)?;
                return Ok(false);
            }
        }
        Ok(true)
    }
}
