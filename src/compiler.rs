use crate::ast::{Binding, Expr, ExprKind, Initializer, Name, Span, Stmt};
use crate::backend::ir::{Body, Call, Instruction, Value};
use std::collections::{BTreeMap, BTreeSet};
mod callables;
mod generics;
mod higher_order;
mod records;
mod tuples;

#[derive(Clone)]
struct Function {
    name: Name,
    params: Vec<String>,
    result: String,
    cpp: String,
    code: String,
    body: Option<(Vec<Name>, Box<Stmt>)>,
}
#[derive(Clone)]
struct Variable {
    cpp: String,
    ty: String,
    initialized: bool,
}
#[derive(Default)]
pub struct Compiler {
    types: BTreeMap<String, String>,
    callables: BTreeMap<String, callables::Callable>,
    function_types: BTreeMap<usize, String>,
    lambda_types: BTreeMap<String, String>,
    iterator_types: BTreeMap<String, higher_order::IteratorType>,
    lambda_counter: usize,
    current_function: Option<usize>,
    records: BTreeMap<String, records::Record>,
    prepared_records: BTreeSet<String>,
    record_copies: BTreeMap<String, Vec<usize>>,
    generic_records: BTreeMap<String, generics::GenericRecord>,
    generic_functions: Vec<generics::GenericFunction>,
    generic_instances: BTreeMap<(usize, Vec<String>), usize>,
    generic_calls: BTreeMap<usize, Span>,
    functions: Vec<Function>,
    symbols: BTreeMap<String, String>,
    scopes: Vec<BTreeMap<String, Variable>>,
    body: Body,
    no_optimization: bool,
    counter: usize,
    used: BTreeSet<usize>,
    loops: Vec<Option<String>>,
    return_type: Option<String>,
    pending_temps: Vec<String>,
}
fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            32..=126 => out.push(char::from(b)),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('"');
    out
}
pub(crate) fn location(s: &Span) -> String {
    format!("Loc{{{}, {}, {}}}", quoted(&s.file), s.line, s.column)
}
fn property(n: &Name, p: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    p.get(key)
        .cloned()
        .ok_or_else(|| n.span.error(format!("missing property '{key}'")))
}
fn validate(n: &Name, p: &BTreeMap<String, String>, allowed: &[&str]) -> Result<(), String> {
    for key in p.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(n.span.error(format!("unknown property '{key}'")));
        }
    }
    Ok(())
}
impl Compiler {
    pub fn with_optimization(enabled: bool) -> Self {
        Self {
            no_optimization: !enabled,
            ..Self::default()
        }
    }
    pub fn compile(mut self, sources: &[Vec<Stmt>]) -> Result<String, String> {
        for source in sources {
            for s in source {
                if let Stmt::UserFunction(name, parameters, arguments, result, body) = s
                    && !parameters.is_empty()
                {
                    self.register_generic_function(generics::GenericFunction {
                        name: name.clone(),
                        parameters: parameters.clone(),
                        arguments: arguments.clone(),
                        result: result.clone(),
                        body: body.clone(),
                    })?;
                }
                if let Stmt::Struct(name, parameters, fields) = s {
                    if name.text.starts_with("__Rusk")
                        || matches!(name.text.as_str(), "Vec" | "Tuple" | "Iter")
                    {
                        return Err(name.span.error("reserved built-in type constructor"));
                    }
                    if !parameters.is_empty() {
                        self.register_generic_record(generics::GenericRecord {
                            name: name.clone(),
                            parameters: parameters.clone(),
                            fields: fields.clone(),
                        })?;
                        continue;
                    }
                    let native = format!("RuskRecord{}", self.records.len());
                    if self.generic_records.contains_key(&name.text)
                        || self.types.insert(name.text.clone(), native).is_some()
                    {
                        return Err(name.span.error("duplicate type"));
                    }
                    self.records.insert(
                        name.text.clone(),
                        records::Record {
                            name: name.clone(),
                            fields: fields.clone(),
                        },
                    );
                }
                if let Stmt::Type(n, p) = s {
                    if n.text.starts_with("__Rusk") {
                        return Err(n.span.error("reserved internal type name"));
                    }
                    if self.generic_records.contains_key(&n.text) {
                        return Err(n.span.error("duplicate type"));
                    }
                    validate(n, p, &["cpp_name"])?;
                    if self
                        .types
                        .insert(n.text.clone(), property(n, p, "cpp_name")?)
                        .is_some()
                    {
                        return Err(n.span.error("duplicate type"));
                    }
                }
            }
        }
        self.prepare_records()?;
        for source in sources {
            for s in source {
                if let Stmt::UserFunction(_, parameters, ..) = s
                    && !parameters.is_empty()
                {
                    continue;
                }
                match s {
                    Stmt::Function(n, params, result, _)
                    | Stmt::UserFunction(n, _, params, result, _) => {
                        let (cpp, code, body) = match s {
                            Stmt::Function(_, _, _, p) => {
                                validate(n, p, &["cpp_name", "code"])?;
                                (property(n, p, "cpp_name")?, property(n, p, "code")?, None)
                            }
                            Stmt::UserFunction(_, _, params, _, body) => (
                                format!("rusk_fn{}", self.functions.len()),
                                String::new(),
                                Some((
                                    params.iter().map(|(n, _)| n.clone()).collect(),
                                    body.clone(),
                                )),
                            ),
                            _ => unreachable!(),
                        };
                        let mut names = BTreeSet::new();
                        for (param, ty) in params {
                            self.check_type(ty)?;
                            if !names.insert(&param.text) {
                                return Err(param.span.error("duplicate parameter"));
                            }
                        }
                        if let Some(r) = result {
                            self.check_type(r)?;
                        }
                        let result = result
                            .as_ref()
                            .map(|r| r.text.clone())
                            .unwrap_or("Unit".into());
                        let params: Vec<_> = params.iter().map(|(_, t)| t.text.clone()).collect();
                        if let Some(old) = self.functions.iter().find(|f| {
                            f.name.text == n.text
                                && f.params == params
                                && (!return_selected(&n.text) || f.result == result)
                        }) {
                            return Err(n.span.error(format!(
                                "duplicate function; previous definition at {}:{}:{}",
                                old.name.span.file, old.name.span.line, old.name.span.column
                            )));
                        }
                        self.functions.push(Function {
                            name: n.clone(),
                            params,
                            result,
                            cpp,
                            code,
                            body,
                        });
                    }
                    Stmt::Symbol(n, p) => {
                        validate(n, p, &["code"])?;
                        if self
                            .symbols
                            .insert(n.text.clone(), property(n, p, "code")?)
                            .is_some()
                        {
                            return Err(n.span.error("duplicate symbol"));
                        }
                    }
                    _ => {}
                }
            }
        }
        // Check every body, including unused functions, in an isolated variable scope.
        for i in 0..self.functions.len() {
            if self.functions[i].body.is_some() {
                self.compile_function(i)?;
            }
        }
        self.scopes.push(BTreeMap::new());
        for source in sources {
            for s in source {
                self.statement(s)?;
            }
        }
        let main_body = std::mem::take(&mut self.body);
        let mut index = 0;
        loop {
            while index < self.functions.len() {
                if self.functions[index].body.is_some()
                    && self.functions[index].code.is_empty()
                    && let Err(error) = self.compile_function(index)
                {
                    if let Some(span) = self.generic_calls.get(&index) {
                        return Err(format!(
                            "{error}\nwhile instantiating '{}' at {}:{}:{}",
                            self.generic_instances
                                .iter()
                                .find(|(_, value)| **value == index)
                                .map(|((template, arguments), _)| format!(
                                    "{}<{}>",
                                    self.generic_functions[*template].name.text,
                                    arguments.join(",")
                                ))
                                .unwrap_or_else(|| self.functions[index].name.text.clone()),
                            span.file,
                            span.line,
                            span.column
                        ));
                    }
                    return Err(error);
                }
                index += 1;
            }
            self.prepare_record_copies()?;
            if index == self.functions.len() && self.record_copies.len() == self.records.len() {
                break;
            }
        }
        self.body = main_body;
        let mut out =
            String::from("// Generated by Rusk beta. C++20, GCC/Clang checked arithmetic.\n");
        for code in self.symbols.values() {
            out.push_str(code);
            out.push('\n');
        }
        out.push_str(&self.lazy_cpp());
        out.push_str(&self.record_cpp());
        out.push_str(&self.lazy_copy_declarations());
        for f in &self.functions {
            if f.body.is_some() {
                out.push_str(&self.signature(f));
                out.push_str(";\n");
            }
        }
        for i in &self.used {
            if self.functions[*i].body.is_none() {
                out.push_str(&self.functions[*i].code);
                out.push('\n');
            }
        }
        for f in &self.functions {
            if f.body.is_some() {
                out.push_str(&f.code);
                out.push('\n');
            }
        }
        out.push_str(&self.record_copy_cpp());
        out.push_str(&self.lazy_copy_cpp());
        out.push_str("int main() {\n");
        out.push_str(&self.body.cpp(!self.no_optimization));
        out.push_str("return 0;\n}\n");
        Ok(out)
    }
    fn signature(&self, f: &Function) -> String {
        let result = if f.result == "Unit" {
            "void".into()
        } else {
            format!("Obj<{}>", self.types[&f.result])
        };
        let mut params: Vec<_> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, ty)| format!("Obj<{}> rusk_p{i}", self.types[ty]))
            .collect();
        params.push("[[maybe_unused]] Loc rusk_call_loc".into());
        format!("{result} {}({})", f.cpp, params.join(", "))
    }
    fn compile_function(&mut self, index: usize) -> Result<(), String> {
        self.current_function = Some(index);
        let f = self.functions[index].clone();
        let (params, body) = f.body.as_ref().unwrap();
        if f.result != "Unit" && !definitely_returns(body) {
            return Err(f
                .name
                .span
                .error("function may finish without returning a value"));
        }
        self.return_type = Some(f.result.clone());
        self.scopes = vec![BTreeMap::new()];
        self.body.clear();
        for (i, (param, ty)) in params.iter().zip(&f.params).enumerate() {
            self.declare(
                param,
                ty.clone(),
                Some(Value {
                    cpp: format!("rusk_p{i}"),
                    ty: ty.clone(),
                }),
            )?;
        }
        let Stmt::Block(statements) = body.as_ref() else {
            unreachable!()
        };
        for statement in statements {
            self.statement(statement)?;
        }
        self.functions[index].code = format!(
            "{} {{\n{}{}\n}}\n",
            self.signature(&f),
            self.body.cpp(!self.no_optimization),
            if f.result == "Unit" { "return;" } else { "" }
        );
        self.scopes.clear();
        self.body.clear();
        self.return_type = None;
        self.current_function = None;
        Ok(())
    }
    fn check_type(&mut self, n: &Name) -> Result<(), String> {
        if self.types.contains_key(&n.text) {
            Ok(())
        } else if n.text.starts_with("Tuple<") {
            self.specialize_tuple(n)
        } else if let Some(inner) = vector_element(&n.text) {
            let inner = inner.to_string();
            self.check_type(&Name {
                text: inner.clone(),
                span: n.span.clone(),
            })?;
            if !matches!(inner.as_str(), "Int" | "Bool" | "Char" | "Str")
                && vector_element(&inner).is_none()
                && !self.records.contains_key(&inner)
            {
                return Err(n
                    .span
                    .error("vectors require basic, struct or vector element types"));
            }
            self.specialize_vector(&n.text, &inner)?;
            Ok(())
        } else {
            if self.specialize_record(n)? {
                Ok(())
            } else {
                Err(n.span.error(format!("unknown type '{}'", n.text)))
            }
        }
    }
    fn specialize_vector(&mut self, ty: &str, inner: &str) -> Result<(), String> {
        let native_element = self.types[inner].clone();
        let native = format!("RuskVec<{native_element}>");
        let iterator = format!("Iter<{ty}>");
        let native_iterator = format!("RuskIter<{native_element}>");
        self.types.insert(ty.into(), native.clone());
        self.types.insert(iterator.clone(), native_iterator.clone());
        let prefix = format!("vector_{}_", self.functions.len());
        let source = include_str!("../lib/vec.rusk")
            .replace("NativeIterator", &native_iterator)
            .replace("NativeElement", &native_element)
            .replace("NativeVec", &native)
            .replace("Iterator", &iterator)
            .replace("Element", inner)
            .replace("Vector", ty)
            .replace("vector_function_", &prefix);
        for statement in crate::parse(&source, "<stdlib/vec.rusk>")? {
            if let Stmt::Function(name, params, result, properties) = statement {
                if name.text == "__read__"
                    && (vector_element(inner).is_some() || self.records.contains_key(inner))
                {
                    continue;
                }
                if self.record_element(inner)
                    && matches!(
                        name.text.as_str(),
                        "__write__"
                            | "sort"
                            | "sorted"
                            | "bisect_left"
                            | "bisect_right"
                            | "contains"
                            | "__eq__"
                            | "__ne__"
                            | "__lt__"
                            | "__le__"
                            | "__gt__"
                            | "__ge__"
                    )
                {
                    continue;
                }
                self.functions.push(Function {
                    cpp: property(&name, &properties, "cpp_name")?,
                    code: property(&name, &properties, "code")?,
                    name,
                    params: params.into_iter().map(|(_, ty)| ty.text).collect(),
                    result: result.map_or("Unit".into(), |ty| ty.text),
                    body: None,
                });
            }
        }
        Ok(())
    }
    fn temp(&mut self) -> String {
        let r = format!("rusk_v{}", self.counter);
        self.counter += 1;
        r
    }
    fn emit(&mut self, ty: &str, code: String) -> Value {
        let cpp = self.temp();
        let value = Value {
            cpp: cpp.clone(),
            ty: ty.into(),
        };
        self.body.instructions.push(Instruction::Object {
            result: value.clone(),
            native: self.types[ty].clone(),
            expression: code,
        });
        self.pending_temps.push(cpp);
        value
    }
    fn find_var(&self, n: &Name) -> Result<Variable, String> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(&n.text))
            .cloned()
            .ok_or_else(|| n.span.error(format!("unknown variable '{}'", n.text)))
    }
    fn declare(&mut self, n: &Name, ty: String, value: Option<Value>) -> Result<(), String> {
        if self.scopes.last().unwrap().contains_key(&n.text) {
            return Err(n.span.error(format!("duplicate variable '{}'", n.text)));
        }
        if let Some(v) = &value {
            self.same(&ty, &v.ty, &n.span)?;
        }
        let cpp = self.temp();
        self.body.push_str(&format!(
            "Obj<{}> {cpp}{};\n",
            self.types[&ty],
            value
                .as_ref()
                .map(|v| format!(" = {}", v.cpp))
                .unwrap_or_default()
        ));
        self.scopes.last_mut().unwrap().insert(
            n.text.clone(),
            Variable {
                cpp,
                ty,
                initialized: value.is_some(),
            },
        );
        Ok(())
    }
    fn same(&self, a: &str, b: &str, s: &Span) -> Result<(), String> {
        if a == b {
            Ok(())
        } else {
            Err(s.error(format!("type mismatch: expected {a}, found {b}")))
        }
    }
    fn select(
        &self,
        name: &str,
        args: &[Value],
        expected: Option<&str>,
        s: &Span,
    ) -> Result<Option<usize>, String> {
        let ids: Vec<_> = self
            .functions
            .iter()
            .enumerate()
            .filter(|(index, f)| {
                !self.generic_calls.contains_key(index)
                    && f.name.text == name
                    && f.params == args.iter().map(|v| v.ty.clone()).collect::<Vec<_>>()
                    && (!return_selected(name) || expected.is_none_or(|t| f.result == t))
            })
            .map(|(i, _)| i)
            .collect();
        if ids.len() > 1 {
            return Err(s.error(format!("ambiguous call to '{name}'")));
        }
        Ok(ids.first().copied())
    }
    fn call(
        &mut self,
        name: &str,
        args: Vec<Value>,
        expected: Option<&str>,
        s: &Span,
    ) -> Result<Value, String> {
        let resolved = self.resolve(name, &args, expected, s)?;
        let resolved = if resolved.is_some() {
            resolved
        } else {
            self.higher_order(name, &args, s)?
        };
        let i = resolved.ok_or_else(|| {
            s.error(format!(
                "no matching function '{name}' for ({})",
                args.iter()
                    .map(|v| v.ty.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        self.used.insert(i);
        let f = self.functions[i].clone();
        let result = Value {
            cpp: self.temp(),
            ty: f.result.clone(),
        };
        self.pending_temps.push(result.cpp.clone());
        self.body.instructions.push(Instruction::Call(Call {
            name: f.name.text,
            cpp: f.cpp,
            trusted: f.body.is_none()
                && matches!(
                    f.name.span.file.as_str(),
                    "<stdlib/core.rusk>" | "<stdlib/vec.rusk>"
                ),
            args,
            span: s.clone(),
            result: result.clone(),
            native: self.types[&f.result].clone(),
        }));
        Ok(result)
    }

    fn literal(&mut self, ty: &str, text: &str, s: &Span) -> Result<Value, String> {
        let cpp = self
            .types
            .get(ty)
            .ok_or_else(|| s.error(format!("literal type '{ty}' is not defined")))?
            .clone();
        let value = match ty {
            "Int" => {
                let v = text
                    .parse::<i64>()
                    .map_err(|_| s.error("integer literal out of range"))?;
                if v == i64::MIN {
                    "(-INT64_C(9223372036854775807)-1)".into()
                } else {
                    format!("INT64_C({v})")
                }
            }
            "Str" => {
                let v: String = serde_json::from_str(text).map_err(|e| s.error(e))?;
                if !v.is_ascii() {
                    return Err(s.error("Str currently requires ASCII"));
                }
                format!("std::string({}, {})", quoted(&v), v.len())
            }
            "Char" => {
                let inner = &text[1..text.len() - 1];
                let c = if inner.starts_with('\\') {
                    match inner {
                        "\\n" => b'\n',
                        "\\r" => b'\r',
                        "\\t" => b'\t',
                        "\\0" => 0,
                        "\\\\" => b'\\',
                        "\\'" => b'\'',
                        "\\\"" => b'"',
                        _ => return Err(s.error("unsupported character escape")),
                    }
                } else if inner.len() == 1 && inner.is_ascii() {
                    inner.as_bytes()[0]
                } else {
                    return Err(s.error("Char requires one ASCII character"));
                };
                format!("static_cast<char>({c})")
            }
            "Bool" => text.into(),
            _ => return Err(s.error("unsupported literal")),
        };
        let result = Value {
            cpp: self.temp(),
            ty: ty.into(),
        };
        self.pending_temps.push(result.cpp.clone());
        self.body.instructions.push(Instruction::Literal {
            result: result.clone(),
            native: cpp,
            expression: value,
        });
        Ok(result)
    }
    fn release_temps(&mut self) {
        for temp in self.pending_temps.drain(..) {
            self.body.push_str(&format!("{temp}.reset();\n"));
        }
    }
    fn statement(&mut self, s: &Stmt) -> Result<(), String> {
        let outer = std::mem::take(&mut self.pending_temps);
        self.statement_inner(s)?;
        self.release_temps();
        self.pending_temps = outer;
        Ok(())
    }
    fn statement_inner(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Include(_, span) => {
                return Err(span.error("include is only allowed at top level"));
            }
            Stmt::Type(n, _)
            | Stmt::Struct(n, ..)
            | Stmt::Function(n, ..)
            | Stmt::UserFunction(n, ..)
            | Stmt::Symbol(n, _) => {
                if self.return_type.is_some() || self.scopes.len() != 1 {
                    return Err(n
                        .span
                        .error("type and function declarations are currently top-level only"));
                }
            }
            Stmt::Return(span, value) => {
                let expected = self
                    .return_type
                    .clone()
                    .ok_or_else(|| span.error("return outside a function"))?;
                if let Some(value) = value {
                    let value = self.expr(value, Some(&expected))?;
                    self.same(&expected, &value.ty, span)?;
                    if expected == "Unit" {
                        self.body.push_str("return;\n");
                    } else {
                        self.body.push_str(&format!("return {};\n", value.cpp));
                    }
                } else {
                    self.same("Unit", &expected, span)?;
                    self.body.push_str("return;\n");
                }
            }
            Stmt::Let(n, t, e) => {
                if let Some(t) = t {
                    self.check_type(t)?;
                }
                let value = e
                    .as_ref()
                    .map(|initializer| {
                        let (e, copied) = match initializer {
                            Initializer::Shared(e) => (e, false),
                            Initializer::Copied(e) => (e, true),
                        };
                        let value = self.expr(e, t.as_ref().map(|n| n.text.as_str()))?;
                        if copied {
                            self.call("copy", vec![value], None, &e.span)
                        } else {
                            Ok(value)
                        }
                    })
                    .transpose()?;
                let ty = t
                    .as_ref()
                    .map(|n| n.text.clone())
                    .or_else(|| value.as_ref().map(|v| v.ty.clone()))
                    .ok_or_else(|| n.span().error("uninitialized declaration requires a type"))?;
                if let Some(value) = value {
                    self.same(&ty, &value.ty, n.span())?;
                    self.declare_binding(n, value)?;
                } else if let Binding::Name(name) = n {
                    self.declare(name, ty, None)?;
                } else {
                    return Err(n.span().error("tuple binding requires an initializer"));
                }
            }
            Stmt::Input(items) => {
                for (n, t, args) in items {
                    self.check_type(t)?;
                    let mut values = vec![];
                    for arg in args {
                        values.push(self.expr(arg, Some("Int"))?);
                    }
                    let v = self.call("__read__", values, Some(&t.text), &n.span)?;
                    self.declare(n, t.text.clone(), Some(v))?;
                }
            }
            Stmt::Output(items) => {
                let span = items.first().map(|e| e.span.clone()).unwrap_or(Span {
                    file: "<output>".into(),
                    line: 1,
                    column: 1,
                });
                for (i, e) in items.iter().enumerate() {
                    if i > 0 {
                        let space = self.literal("Char", "' '", &span)?;
                        self.call("__write__", vec![space], None, &span)?;
                    }
                    let v = self.expr(e, None)?;
                    self.call("__write__", vec![v], None, &e.span)?;
                }
                let newline = self.literal("Char", "'\\n'", &span)?;
                self.call("__write__", vec![newline], None, &span)?;
            }
            Stmt::If(cond, yes, no) => {
                let cond = self.condition(cond)?;
                let before = self.scopes.clone();
                self.body.push_str(&format!("if (*{}) {{\n", cond.cpp));
                self.statement(yes)?;
                let yes_state = self.scopes.clone();
                self.body.push_str("} else {\n");
                self.scopes = before;
                if let Some(no) = no {
                    self.statement(no)?;
                }
                self.body.push_str("}\n");
                if definitely_returns(yes) {
                    // Only the other branch reaches the following statement.
                } else if no.as_ref().is_some_and(|no| definitely_returns(no)) {
                    self.scopes = yes_state;
                } else {
                    self.intersect_initialization(&yes_state);
                }
            }
            Stmt::While(cond, body, otherwise) => {
                let broken = otherwise.as_ref().map(|_| self.temp());
                if let Some(flag) = &broken {
                    self.body.push_str(&format!("bool {flag} = false;\n"));
                }
                self.body.push_str("while (true) {\n");
                let outer_temps = std::mem::take(&mut self.pending_temps);
                let value = self.condition(cond)?;
                let before_body = self.scopes.clone();
                self.body
                    .push_str(&format!("if (!*{}) break;\n", value.cpp));
                self.release_temps();
                self.pending_temps = outer_temps;
                self.loops.push(broken.clone());
                self.statement(body)?;
                self.loops.pop();
                self.body.push_str("}\n");
                // The body may execute zero times. The first condition always executes.
                self.scopes = before_body;
                if let Some(otherwise) = otherwise {
                    let before_else = self.scopes.clone();
                    self.body
                        .push_str(&format!("if (!{}) {{\n", broken.unwrap()));
                    self.statement(otherwise)?;
                    self.body.push_str("}\n");
                    self.scopes = before_else;
                }
            }
            Stmt::For(name, source, body, otherwise) => {
                let source = self.expr(source, None)?;
                let iterator = self.call("__iter__", vec![source], None, name.span())?;
                let before = self.scopes.clone();
                let broken = otherwise.as_ref().map(|_| self.temp());
                if let Some(flag) = &broken {
                    self.body.push_str(&format!("bool {flag}=false;\n"));
                }
                self.body.push_str("while (true) {\n");
                // Loop-local temporaries must be destroyed before their C++ scope ends.
                let outer_temps = std::mem::take(&mut self.pending_temps);
                let has_next = self.call(
                    "__iter_has_next__",
                    vec![iterator.clone()],
                    None,
                    name.span(),
                )?;
                self.same("Bool", &has_next.ty, name.span())?;
                self.body
                    .push_str(&format!("if (!*{}) break;\n", has_next.cpp));
                self.scopes.push(BTreeMap::new());
                let value = self.call("__iter_next__", vec![iterator], None, name.span())?;
                self.declare_binding(name, value)?;
                self.release_temps();
                self.loops.push(broken.clone());
                let Stmt::Block(statements) = body.as_ref() else {
                    unreachable!()
                };
                for statement in statements {
                    self.statement(statement)?;
                }
                self.loops.pop();
                self.scopes.pop();
                self.pending_temps = outer_temps;
                self.body.push_str("}\n");
                self.scopes = before;
                if let Some(otherwise) = otherwise {
                    let before_else = self.scopes.clone();
                    self.body
                        .push_str(&format!("if (!{}) {{\n", broken.unwrap()));
                    self.statement(otherwise)?;
                    self.body.push_str("}\n");
                    self.scopes = before_else;
                }
            }
            Stmt::Loop(body) => {
                let before = self.scopes.clone();
                self.body.push_str("while (true) {\n");
                self.loops.push(None);
                self.statement(body)?;
                self.loops.pop();
                self.body.push_str("}\n");
                self.scopes = before;
            }
            Stmt::Break(span) => {
                let flag = self
                    .loops
                    .last()
                    .ok_or_else(|| span.error("break outside a loop"))?;
                if let Some(flag) = flag {
                    self.body.push_str(&format!("{flag} = true;\n"));
                }
                self.body.push_str("break;\n");
            }
            Stmt::Continue(span) => {
                if self.loops.is_empty() {
                    return Err(span.error("continue outside a loop"));
                }
                self.body.push_str("continue;\n");
            }
            Stmt::Block(body) => {
                self.scopes.push(BTreeMap::new());
                self.body.push_str("{\n");
                for s in body {
                    if let Stmt::Type(n, _) | Stmt::Function(n, ..) | Stmt::Symbol(n, _) = s {
                        return Err(n
                            .span
                            .error("extern declarations are currently top-level only"));
                    }
                    self.statement(s)?;
                }
                self.body.push_str("}\n");
                self.scopes.pop();
            }
            Stmt::Expr(e) => {
                self.expr(e, None)?;
            }
        }
        Ok(())
    }
    fn condition(&mut self, expr: &Expr) -> Result<Value, String> {
        let outer = std::mem::take(&mut self.pending_temps);
        let value = self.expr(expr, None)?;
        let result = self.call("__bool__", vec![value], None, &expr.span)?;
        self.same("Bool", &result.ty, &expr.span)?;
        self.pending_temps.retain(|name| name != &result.cpp);
        self.release_temps();
        self.pending_temps = outer;
        self.pending_temps.push(result.cpp.clone());
        Ok(result)
    }
    fn intersect_initialization(&mut self, other: &[BTreeMap<String, Variable>]) {
        for (scope, other_scope) in self.scopes.iter_mut().zip(other) {
            for (name, variable) in scope {
                variable.initialized &= other_scope[name].initialized;
            }
        }
    }
    fn expr(&mut self, e: &Expr, expected: Option<&str>) -> Result<Value, String> {
        match &e.kind {
            ExprKind::Lambda(parameters, body) => self.lambda(parameters, body, &e.span),
            ExprKind::Invoke(callee, args) => self.invoke_expression(callee, args, &e.span),
            ExprKind::Tuple(items) => self.tuple_value(items, expected, &e.span),
            ExprKind::Record(name, fields) => self.record_value(name, fields),
            ExprKind::Field(receiver, field) => {
                let receiver = self.expr(receiver, None)?;
                if self.callables.contains_key(&receiver.ty) {
                    return Err(field.span.error("callable environment fields are private"));
                }
                let (index, ty) = self.field(&receiver.ty, field)?;
                Ok(self.emit(&ty, format!("{}->field_{index}", receiver.cpp)))
            }
            ExprKind::Literal(ty, text) => self.literal(ty, text, &e.span),
            ExprKind::Array(items) => {
                let hint = expected.and_then(vector_element);
                let mut values = vec![];
                for item in items {
                    values.push(self.expr(item, hint)?);
                }
                let element = values
                    .first()
                    .map(|v| v.ty.as_str())
                    .or(hint)
                    .ok_or_else(|| e.span.error("empty array requires a vector type context"))?
                    .to_string();
                for value in &values {
                    self.same(&element, &value.ty, &e.span)?;
                }
                let ty = format!("Vec<{element}>");
                self.check_type(&Name {
                    text: ty.clone(),
                    span: e.span.clone(),
                })?;
                let array = self.call("__vec_new__", vec![], Some(&ty), &e.span)?;
                for value in values {
                    self.call("push_back", vec![array.clone(), value], None, &e.span)?;
                }
                Ok(array)
            }
            ExprKind::Repeat(initial, count) => {
                let initial = self.expr(initial, expected.and_then(vector_element))?;
                let ty = format!("Vec<{}>", initial.ty);
                self.check_type(&Name {
                    text: ty.clone(),
                    span: e.span.clone(),
                })?;
                let count = self.expr(count, Some("Int"))?;
                self.call("__vec_repeat__", vec![initial, count], Some(&ty), &e.span)
            }
            ExprKind::Index(array, index) => {
                let array = self.expr(array, None)?;
                if array.ty.starts_with("Tuple<") {
                    let index_value = tuples::literal_index(index)?;
                    return self.tuple_item(&array, index_value, &index.span);
                }
                let index = self.expr(index, Some("Int"))?;
                self.call("__getitem__", vec![array, index], None, &e.span)
            }
            ExprKind::Range(start, end) => {
                let start = self.expr(start, Some("Int"))?;
                let end = self.expr(end, Some("Int"))?;
                self.call("__range__", vec![start, end], None, &e.span)
            }
            ExprKind::Logical(op, left, right) => {
                let left = self.condition(left)?;
                let result = self.emit("Bool", format!("make_obj<bool>(*{})", left.cpp));
                let before = self.scopes.clone();
                self.body.push_str(&format!(
                    "if ({}*{}) {{\n",
                    if op == "||" { "!" } else { "" },
                    left.cpp
                ));
                let outer_temps = std::mem::take(&mut self.pending_temps);
                let right = self.condition(right)?;
                self.body
                    .push_str(&format!("*{}=*{};\n", result.cpp, right.cpp));
                self.release_temps();
                self.pending_temps = outer_temps;
                self.body.push_str("}\n");
                self.scopes = before;
                Ok(result)
            }
            ExprKind::Var(n) => {
                let Some(v) = self.local_variable(&n.text) else {
                    return self.function_value(n);
                };
                if !v.initialized {
                    return Err(n.span.error("variable used before initialization"));
                }
                Ok(self.emit(&v.ty, v.cpp))
            }
            ExprKind::Call(n, args) => {
                if self
                    .local_variable(&n.text)
                    .is_some_and(|v| self.callables.contains_key(&v.ty))
                {
                    let callee = self.expr(
                        &Expr {
                            kind: ExprKind::Var(n.clone()),
                            span: n.span.clone(),
                        },
                        None,
                    )?;
                    return self.invoke_arguments(callee, args, &e.span);
                }
                let mut values = vec![];
                for arg in args {
                    // Use a context only when every remaining overload agrees.
                    let hints = self.argument_hints(&n.text, args.len(), &values);
                    let hint = if hints.len() == 1 {
                        hints.first().map(String::as_str)
                    } else {
                        None
                    };
                    values.push(self.expr(arg, hint)?);
                }
                self.call(&n.text, values, expected, &e.span)
            }
            ExprKind::Binary(op, a, b) => {
                let a = self.expr(a, None)?;
                let b = self.expr(b, None)?;
                self.call(binary_name(op), vec![a, b], None, &e.span)
            }
            ExprKind::Unary(op, x) => {
                // The minimum Int is spelled as a negated positive magnitude.
                if op == "-"
                    && let ExprKind::Literal(ty, text) = &x.kind
                    && ty == "Int"
                    && text == "9223372036854775808"
                {
                    return self.literal("Int", "-9223372036854775808", &e.span);
                }
                let x = self.expr(x, None)?;
                self.call(
                    match op.as_str() {
                        "-" => "__neg__",
                        "!" => "__not__",
                        _ => "__pos__",
                    },
                    vec![x],
                    None,
                    &e.span,
                )
            }
            ExprKind::Assign(op, left, right) => {
                if matches!(left.kind, ExprKind::Tuple(_)) {
                    if op != "=" && op != ":=" {
                        return Err(e.span.error("tuple assignment only supports = and :="));
                    }
                    let mut value = self.expr(right, None)?;
                    if op == "=" {
                        value = self.call("copy", vec![value], None, &right.span)?;
                    }
                    return self.assign_tuple_value(op, left, value);
                }
                if op == ":=" {
                    if let ExprKind::Field(receiver, field) = &left.kind {
                        let receiver = self.expr(receiver, None)?;
                        let (index, ty) = self.field(&receiver.ty, field)?;
                        let rhs = self.expr(right, Some(&ty))?;
                        self.same(&ty, &rhs.ty, &right.span)?;
                        self.body
                            .push_str(&format!("{}->field_{index} = {};\n", receiver.cpp, rhs.cpp));
                        return Ok(self.emit(&ty, rhs.cpp));
                    }
                    if let ExprKind::Index(array, index) = &left.kind {
                        let array = self.expr(array, None)?;
                        if array.ty.starts_with("Tuple<") {
                            let position = tuples::literal_index(index)?;
                            let item = self.tuple_item(&array, position, &index.span)?;
                            let rhs = self.expr(right, Some(&item.ty))?;
                            self.same(&item.ty, &rhs.ty, &right.span)?;
                            self.body.push_str(&format!(
                                "{}->field_{position} = {};\n",
                                array.cpp, rhs.cpp
                            ));
                            return Ok(self.emit(&item.ty, rhs.cpp));
                        }
                        let index = self.expr(index, Some("Int"))?;
                        let element = vector_element(&array.ty).map(str::to_string);
                        let rhs = self.expr(right, element.as_deref())?;
                        return self.call("__setitem__", vec![array, index, rhs], None, &e.span);
                    }
                    let ExprKind::Var(n) = &left.kind else {
                        return Err(e
                            .span
                            .error("rebinding requires a variable, field or index access"));
                    };
                    let v = self.find_var(n)?;
                    let r = self.expr(right, Some(&v.ty))?;
                    self.same(&v.ty, &r.ty, &e.span)?;
                    self.body.push_str(&format!("{} = {};\n", v.cpp, r.cpp));
                    for scope in self.scopes.iter_mut().rev() {
                        if let Some(v) = scope.get_mut(&n.text) {
                            v.initialized = true;
                            break;
                        }
                    }
                    return Ok(self.emit(&v.ty, v.cpp));
                }
                let target = self.expr(left, None)?;
                let rhs = self.expr(right, Some(&target.ty))?;
                if op == "=" {
                    self.call("__assign__", vec![target.clone(), rhs], None, &e.span)?;
                } else {
                    let binary = &op[..1];
                    let inplace = format!("__i{}", &binary_name(binary)[2..]);
                    if self
                        .resolve(&inplace, &[target.clone(), rhs.clone()], None, &e.span)?
                        .is_some()
                    {
                        self.call(&inplace, vec![target.clone(), rhs], None, &e.span)?;
                    } else {
                        let result = self.call(
                            binary_name(binary),
                            vec![target.clone(), rhs],
                            None,
                            &e.span,
                        )?;
                        self.call("__assign__", vec![target.clone(), result], None, &e.span)?;
                    }
                }
                Ok(target)
            }
        }
    }
}
fn binary_name(op: &str) -> &str {
    match op {
        "&" => "__bitand__",
        "|" => "__bitor__",
        "^" => "__bitxor__",
        "+" => "__add__",
        "-" => "__sub__",
        "*" => "__mul__",
        "/" => "__div__",
        "%" => "__mod__",
        "==" => "__eq__",
        "!=" => "__ne__",
        "<" => "__lt__",
        "<=" => "__le__",
        ">" => "__gt__",
        ">=" => "__ge__",
        _ => unreachable!(),
    }
}

fn definitely_returns(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return(..) => true,
        Stmt::Block(statements) => statements.iter().any(definitely_returns),
        Stmt::If(_, yes, Some(no)) => definitely_returns(yes) && definitely_returns(no),
        _ => false,
    }
}

fn vector_element(ty: &str) -> Option<&str> {
    ty.strip_prefix("Vec<")?.strip_suffix('>')
}
fn return_selected(name: &str) -> bool {
    matches!(name, "__read__" | "__vec_new__" | "__vec_repeat__")
}
