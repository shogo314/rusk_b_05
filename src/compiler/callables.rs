use super::*;

#[derive(Clone)]
pub(super) struct Callable {
    pub parameters: Vec<String>,
    pub result: String,
    pub function: usize,
}

fn references(expr: &Expr, bound: &BTreeSet<String>, output: &mut BTreeMap<String, bool>) {
    match &expr.kind {
        ExprKind::Block(statements, tail) => {
            let mut local = bound.clone();
            for statement in statements {
                statement_references(statement, &mut local, output);
            }
            if let Some(tail) = tail {
                references(tail, &local, output);
            }
        }
        ExprKind::Match(value, arms) => {
            references(value, bound, output);
            for arm in arms {
                let mut local = bound.clone();
                for b in &arm.bindings {
                    bind_names(b, &mut local);
                }
                references(&arm.body, &local, output);
            }
        }
        ExprKind::If(a, b, c) => {
            references(a, bound, output);
            references(b, bound, output);
            references(c, bound, output);
        }
        ExprKind::CompareChain(first, rest) => {
            references(first, bound, output);
            for (_, value, _) in rest {
                references(value, bound, output);
            }
        }
        ExprKind::Unwrap(value) => references(value, bound, output),
        ExprKind::Coalesce(a, b) => {
            references(a, bound, output);
            references(b, bound, output);
        }
        ExprKind::Var(n) => {
            if !bound.contains(&n.text) && !n.text.contains("::") && n.text != "None" {
                output.insert(n.text.clone(), true);
            }
        }
        ExprKind::Call(n, args) => {
            if !bound.contains(&n.text) {
                output.entry(n.text.clone()).or_insert(false);
            }
            for arg in args {
                references(arg, bound, output);
            }
        }
        ExprKind::Lambda(parameters, body) => {
            let mut local = bound.clone();
            local.extend(parameters.iter().map(|(name, _)| name.text.clone()));
            references(body, &local, output);
        }
        ExprKind::Invoke(callee, args) => {
            references(callee, bound, output);
            for arg in args {
                references(arg, bound, output);
            }
        }
        ExprKind::Array(items) | ExprKind::Tuple(items) => {
            for item in items {
                references(item, bound, output);
            }
        }
        ExprKind::Repeat(a, b)
        | ExprKind::Range(a, b)
        | ExprKind::Index(a, b)
        | ExprKind::Logical(_, a, b)
        | ExprKind::Binary(_, a, b)
        | ExprKind::Assign(_, a, b) => {
            references(a, bound, output);
            references(b, bound, output);
        }
        ExprKind::Field(owner, _) | ExprKind::Unary(_, owner) => references(owner, bound, output),
        ExprKind::Record(_, fields) => {
            for (_, value) in fields {
                references(value, bound, output);
            }
        }
        ExprKind::Literal(..) => {}
    }
}

fn bind_names(binding: &Binding, bound: &mut BTreeSet<String>) {
    match binding {
        Binding::Name(n) => {
            bound.insert(n.text.clone());
        }
        Binding::Tuple(items, _) => {
            for item in items {
                bind_names(item, bound);
            }
        }
    }
}
fn statement_references(
    statement: &Stmt,
    bound: &mut BTreeSet<String>,
    output: &mut BTreeMap<String, bool>,
) {
    match statement {
        Stmt::Let(binding, _, initial) => {
            if let Some(Initializer::Shared(e) | Initializer::Copied(e)) = initial {
                references(e, bound, output);
            }
            bind_names(binding, bound);
        }
        Stmt::Input(items) => {
            for (name, _, args) in items {
                for arg in args {
                    references(arg, bound, output);
                }
                bound.insert(name.text.clone());
            }
        }
        Stmt::Expr(e) | Stmt::Match(e) | Stmt::Return(_, Some(e)) => references(e, bound, output),
        Stmt::Output(items) => {
            for item in items {
                references(item, bound, output);
            }
        }
        Stmt::Block(items, tail) => {
            let mut local = bound.clone();
            for item in items {
                statement_references(item, &mut local, output);
            }
            if let Some(tail) = tail {
                references(tail, &local, output);
            }
        }
        Stmt::If(cond, yes, no) | Stmt::While(cond, yes, no) => {
            references(cond, bound, output);
            statement_references(yes, &mut bound.clone(), output);
            if let Some(no) = no {
                statement_references(no, &mut bound.clone(), output);
            }
        }
        Stmt::For(binding, source, body, otherwise) => {
            references(source, bound, output);
            let mut local = bound.clone();
            bind_names(binding, &mut local);
            statement_references(body, &mut local, output);
            if let Some(otherwise) = otherwise {
                statement_references(otherwise, &mut bound.clone(), output);
            }
        }
        Stmt::Loop(body) | Stmt::Labeled(_, body) => {
            statement_references(body, &mut bound.clone(), output)
        }
        _ => {}
    }
}

impl Compiler {
    pub(super) fn local_variable(&self, name: &str) -> Option<Variable> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).cloned()
    }

    pub(super) fn invoke(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        span: &Span,
    ) -> Result<Value, String> {
        if self.diverged {
            return Ok(Value::never());
        }

        let callable = self
            .callables
            .get(&callee.ty)
            .cloned()
            .ok_or_else(|| span.error(format!("type '{}' is not callable", callee.ty)))?;
        if args.len() != callable.parameters.len() {
            return Err(span.error("callable argument count mismatch"));
        }
        for (value, ty) in args.iter().zip(&callable.parameters) {
            self.same(ty, &value.ty, span)?;
        }
        let name = self.functions[callable.function].name.text.clone();
        let mut values = vec![callee];
        values.extend(args);
        self.call(&name, values, None, span)
    }

    pub(super) fn lambda(
        &mut self,
        parameters: &[(Name, Name)],
        body: &Expr,
        span: &Span,
    ) -> Result<Value, String> {
        let mut bound = BTreeSet::new();
        for (name, ty) in parameters {
            self.check_type(ty)?;
            if !bound.insert(name.text.clone()) {
                return Err(name.span.error("duplicate lambda parameter"));
            }
        }
        let mut names = BTreeMap::new();
        references(body, &bound, &mut names);
        let mut captures = vec![];
        for (text, value_reference) in names {
            if let Some(variable) = self.local_variable(&text)
                && (value_reference || self.callables.contains_key(&variable.ty))
            {
                if !variable.initialized {
                    return Err(span.error(format!(
                        "captured variable '{text}' used before initialization"
                    )));
                }
                captures.push((
                    Name {
                        text,
                        span: span.clone(),
                    },
                    variable,
                ));
            }
        }
        let key = format!(
            "{}:{}:{}:{:?}:{:?}:{:?}",
            span.file,
            span.line,
            span.column,
            self.current_function,
            parameters.iter().map(|(_, t)| &t.text).collect::<Vec<_>>(),
            captures
                .iter()
                .map(|(n, v)| (&n.text, &v.ty))
                .collect::<Vec<_>>()
        );
        let ty = if let Some(ty) = self.lambda_types.get(&key) {
            ty.clone()
        } else {
            if self.callables.len() >= 256 {
                return Err(span.error("callable specialization limit exceeded"));
            }
            let ty = format!("__RuskCallable{}", self.lambda_counter);
            self.lambda_counter += 1;
            let native = format!("RuskRecord{}", self.records.len());
            self.types.insert(ty.clone(), native);
            self.records.insert(
                ty.clone(),
                records::Record {
                    name: Name {
                        text: ty.clone(),
                        span: span.clone(),
                    },
                    fields: captures
                        .iter()
                        .map(|(name, v)| {
                            (
                                name.clone(),
                                Name {
                                    text: v.ty.clone(),
                                    span: span.clone(),
                                },
                            )
                        })
                        .collect(),
                },
            );
            self.prepare_record(&ty)?;
            let saved_body = std::mem::take(&mut self.body);
            let saved_scopes = std::mem::take(&mut self.scopes);
            let saved_temps = std::mem::take(&mut self.pending_temps);
            let saved_loops = std::mem::take(&mut self.loops);
            let saved_return = self.return_type.take();
            let saved_exit = self.diverged;
            let saved_infer = self.infer_return;
            let saved_inferred = self.inferred_return.take();
            self.diverged = false;
            self.infer_return = true;
            self.scopes = vec![BTreeMap::new()];
            self.return_type = Some("Unit".into());
            let checked = (|| {
                for (index, (name, var)) in captures.iter().enumerate() {
                    let captured = self.emit(
                        &var.ty,
                        Rvalue::Field {
                            owner: Value {
                                ty: ty.clone(),
                                id: ValueId::Parameter(0),
                            },
                            index,
                        },
                    );
                    self.declare(name, var.ty.clone(), Some(captured))?;
                }
                for (index, (name, ty)) in parameters.iter().enumerate() {
                    self.declare(
                        name,
                        ty.text.clone(),
                        Some(Value {
                            ty: ty.text.clone(),
                            id: ValueId::Parameter(index + 1),
                        }),
                    )?;
                }
                self.expr(body, None)
            })();
            let mut checked_body = std::mem::take(&mut self.body);
            let exits = self.diverged;
            let inferred = self.inferred_return.take();
            self.body = saved_body;
            self.scopes = saved_scopes;
            self.pending_temps = saved_temps;
            self.loops = saved_loops;
            self.return_type = saved_return;
            self.diverged = saved_exit;
            self.infer_return = saved_infer;
            self.inferred_return = saved_inferred;
            let value = checked?;
            let result = if value.is_never() {
                inferred.clone().unwrap_or("Unit".into())
            } else {
                if let Some(inferred) = &inferred {
                    self.same(inferred, &value.ty, span)?;
                }
                value.ty.clone()
            };
            let index = self.functions.len();
            let f = Function {
                name: Name {
                    text: format!("$invoke{index}"),
                    span: span.clone(),
                },
                params: std::iter::once(ty.clone())
                    .chain(parameters.iter().map(|(_, t)| t.text.clone()))
                    .collect(),
                result: result.clone(),
                cpp: format!("rusk_callable_{index}"),
                code: String::new(),
                body: Some((vec![], Box::new(Stmt::Block(vec![], None)))),
                checked: Some({
                    if !exits {
                        checked_body.push(Instruction::Return(if result == "Unit" {
                            None
                        } else {
                            Some(value.clone())
                        }));
                    }
                    checked_body
                }),
            };
            self.functions.push(f);
            self.callables.insert(
                ty.clone(),
                Callable {
                    parameters: parameters.iter().map(|(_, t)| t.text.clone()).collect(),
                    result,
                    function: index,
                },
            );
            self.lambda_types.insert(key, ty.clone());
            ty
        };
        Ok(self.emit(
            &ty,
            Rvalue::Aggregate(
                captures
                    .into_iter()
                    .map(|(_, v)| Value { id: v.id, ty: v.ty })
                    .collect(),
            ),
        ))
    }

    pub(super) fn function_value(&mut self, name: &Name) -> Result<Value, String> {
        let candidates = self
            .functions
            .iter()
            .enumerate()
            .filter(|(i, f)| f.name.text == name.text && !self.generic_calls.contains_key(i))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(name.span.error(if candidates.is_empty() {
                format!("unknown variable or concrete function '{}'", name.text)
            } else {
                format!(
                    "passing overloaded function '{}' is not supported",
                    name.text
                )
            }));
        }
        let index = candidates[0];
        if let Some(ty) = self.function_types.get(&index).cloned() {
            return Ok(self.emit(&ty, Rvalue::Aggregate(vec![])));
        }
        let f = self.functions[index].clone();
        let parameters = f
            .params
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                (
                    Name {
                        text: format!("$argument{i}"),
                        span: name.span.clone(),
                    },
                    Name {
                        text: ty.clone(),
                        span: name.span.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        let expr = Expr {
            span: name.span.clone(),
            kind: ExprKind::Call(
                name.clone(),
                parameters
                    .iter()
                    .map(|(n, _)| Expr {
                        kind: ExprKind::Var(n.clone()),
                        span: n.span.clone(),
                    })
                    .collect(),
            ),
        };
        let value = self.lambda(&parameters, &expr, &name.span)?;
        self.function_types.insert(index, value.ty.clone());
        Ok(value)
    }
}

impl Compiler {
    pub(super) fn invoke_arguments(
        &mut self,
        callee: Value,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value, String> {
        if self.diverged {
            return Ok(Value::never());
        }

        let info = self
            .callables
            .get(&callee.ty)
            .cloned()
            .ok_or_else(|| span.error(format!("type '{}' is not callable", callee.ty)))?;
        let mut values = vec![];
        for (index, arg) in args.iter().enumerate() {
            values.push(self.expr(arg, info.parameters.get(index).map(String::as_str))?);
        }
        self.invoke(callee, values, span)
    }
    pub(super) fn invoke_expression(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: &Span,
    ) -> Result<Value, String> {
        if let ExprKind::Field(owner, name) = &callee.kind {
            let owner = self.expr(owner, None)?;
            if !self.callables.contains_key(&owner.ty)
                && let Ok((index, ty)) = self.field(&owner.ty, name)
                && self.callables.contains_key(&ty)
            {
                let field = self.emit(&ty, Rvalue::Field { owner, index });
                return self.invoke_arguments(field, args, span);
            }
            if self
                .local_variable(&name.text)
                .is_some_and(|v| self.callables.contains_key(&v.ty))
            {
                let function = self.expr(
                    &Expr {
                        kind: ExprKind::Var(name.clone()),
                        span: name.span.clone(),
                    },
                    None,
                )?;
                let info =
                    self.callables.get(&function.ty).cloned().ok_or_else(|| {
                        span.error("method name refers to a non-callable variable")
                    })?;
                let mut values = vec![owner];
                for (index, arg) in args.iter().enumerate() {
                    values
                        .push(self.expr(arg, info.parameters.get(index + 1).map(String::as_str))?);
                }
                return self.invoke(function, values, span);
            }
            let mut values = vec![owner];
            for arg in args {
                let hints = self.argument_hints(&name.text, args.len() + 1, &values);
                let builtin_hint =
                    self.container_argument_hint(&name.text, args.len() + 1, &values, None);
                let hint = if hints.len() == 1 {
                    hints.first().map(String::as_str)
                } else {
                    builtin_hint.as_deref()
                };
                values.push(self.expr(arg, hint)?);
            }
            return self.call(&name.text, values, None, span);
        }
        let callee = self.expr(callee, None)?;
        self.invoke_arguments(callee, args, span)
    }
}
