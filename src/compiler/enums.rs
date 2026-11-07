use super::tuples::type_parts;
use super::*;

#[derive(Clone)]
pub(super) struct Definition {
    pub name: Name,
    pub parameters: Vec<Name>,
    pub variants: Vec<(Name, Vec<Name>)>,
}
#[derive(Clone)]
pub(super) struct Enum {
    pub definition: String,
    pub arguments: Vec<String>,
    pub variants: Vec<(Name, Vec<Name>)>,
}
impl Compiler {
    pub(super) fn register_enum(
        &mut self,
        name: &Name,
        parameters: &[Name],
        variants: &[(Name, Vec<Name>)],
    ) -> Result<(), String> {
        if self.types.contains_key(&name.text)
            || self.generic_records.contains_key(&name.text)
            || self.enum_definitions.contains_key(&name.text)
        {
            return Err(name.span.error("duplicate type"));
        }
        if name.text.starts_with("__Rusk")
            || matches!(
                name.text.as_str(),
                "Vec" | "Tuple" | "Iter" | "Dict" | "Set"
            )
        {
            return Err(name.span.error("reserved built-in type constructor"));
        }
        if variants.is_empty() {
            return Err(name.span.error("enum requires at least one variant"));
        }
        let mut names = BTreeSet::new();
        for p in parameters {
            if !names.insert(&p.text) {
                return Err(p.span.error("duplicate type parameter"));
            }
        }
        names.clear();
        for (v, _) in variants {
            if !names.insert(&v.text) {
                return Err(v.span.error("duplicate enum variant"));
            }
        }
        self.enum_definitions.insert(
            name.text.clone(),
            Definition {
                name: name.clone(),
                parameters: parameters.to_vec(),
                variants: variants.to_vec(),
            },
        );
        Ok(())
    }
    pub(super) fn specialize_enum(&mut self, name: &Name) -> Result<bool, String> {
        let (base, args) = type_parts(&name.text).unwrap_or((&name.text, vec![]));
        let Some(d) = self.enum_definitions.get(base).cloned() else {
            return Ok(false);
        };
        if args.len() != d.parameters.len() {
            return Err(name.span.error(format!(
                "{base} expects {} type arguments",
                d.parameters.len()
            )));
        }
        self.check_enum_cycle(name, &mut vec![], &mut BTreeSet::new())?;
        if self.preparing_enums.len() >= 64 || self.enums.len() >= 512 {
            return Err(name
                .span
                .error("recursive enum or enum specialization limit exceeded"));
        }
        if self.preparing_enums.contains(&name.text) {
            return Err(name
                .span
                .error("recursive enum types are not implemented yet"));
        }
        for arg in &args {
            self.check_type(&Name {
                text: (*arg).into(),
                span: name.span.clone(),
            })?;
        }
        self.preparing_enums.push(name.text.clone());
        let bindings = d
            .parameters
            .iter()
            .zip(&args)
            .map(|(p, a)| (p.text.clone(), (*a).to_string()))
            .collect();
        let mut variants = d.variants.clone();
        for (_, fields) in &mut variants {
            for field in fields {
                field.text = generics::substitute(&field.text, &bindings);
                self.check_type(field)?;
            }
        }
        self.preparing_enums.pop();
        let native = format!("RuskEnum{}", self.enums.len());
        self.types.insert(name.text.clone(), native.clone());
        self.enums.insert(
            name.text.clone(),
            Enum {
                definition: base.into(),
                arguments: args.iter().map(|s| s.to_string()).collect(),
                variants,
            },
        );
        let ty = &name.text;
        self.native_function(
            "copy",
            vec![ty.clone()],
            ty,
            |cpp| {
                format!("Obj<{native}> {cpp}(Obj<{native}> x,Loc loc){{return deep_copy(x,loc);}}")
            },
            &name.span,
        )?;
        self.native_function("__assign__",vec![ty.clone();2],"Unit",|cpp|format!("void {cpp}(Obj<{native}> x,Obj<{native}> y,Loc loc){{auto snapshot=deep_copy(y,loc);x->value=std::move(snapshot->value);}}"),&name.span)?;
        if base == "Option" {
            let dummy = Value {
                ty: ty.clone(),
                id: ValueId::Probe,
            };
            for op in ["is_some", "is_none", "__bool__", "__not__", "value"] {
                self.option_operation(op, std::slice::from_ref(&dummy), &name.span)?;
            }
        }
        Ok(true)
    }
    fn constructor_parts(&self, name: &str) -> Option<(String, String)> {
        let (owner, variant) = name.rsplit_once("::")?;
        let base = type_parts(owner).map_or(owner, |(b, _)| b);
        self.enum_definitions
            .contains_key(base)
            .then(|| (owner.into(), variant.into()))
    }
    pub(super) fn enum_argument_hint(
        &self,
        name: &str,
        values: &[Value],
        expected: Option<&str>,
    ) -> Option<String> {
        let (owner, variant) = self.constructor_parts(name)?;
        let (base, args) = type_parts(&owner).unwrap_or((&owner, vec![]));
        let d = self.enum_definitions.get(base)?;
        let (_, fields) = d.variants.iter().find(|(v, _)| v.text == variant)?;
        let mut bindings = BTreeMap::new();
        if args.len() == d.parameters.len() {
            bindings.extend(
                d.parameters
                    .iter()
                    .zip(args)
                    .map(|(p, a)| (p.text.clone(), a.into())),
            );
        }
        if let Some(e) = expected {
            let pattern = if d.parameters.is_empty() {
                base.to_string()
            } else {
                format!(
                    "{base}<{}>",
                    d.parameters
                        .iter()
                        .map(|p| p.text.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            generics::unify(&pattern, e, &d.parameters, &mut bindings);
        }
        for (field, value) in fields.iter().zip(values) {
            generics::unify(&field.text, &value.ty, &d.parameters, &mut bindings);
        }
        let hint = generics::substitute(&fields.get(values.len())?.text, &bindings);
        (!d.parameters.iter().any(|p| {
            generics::substitute(&hint, &BTreeMap::from([(p.text.clone(), "?".into())])) != hint
        }))
        .then_some(hint)
    }
    pub(super) fn enum_constructor(
        &mut self,
        name: &str,
        values: &[Value],
        expected: Option<&str>,
        span: &Span,
    ) -> Result<Option<Value>, String> {
        let Some((owner, variant)) = self.constructor_parts(name) else {
            return Ok(None);
        };
        let (base, args) = type_parts(&owner).unwrap_or((&owner, vec![]));
        let d = self.enum_definitions[base].clone();
        let (index, (_, fields)) = d
            .variants
            .iter()
            .enumerate()
            .find(|(_, (v, _))| v.text == variant)
            .ok_or_else(|| span.error(format!("unknown enum variant '{variant}'")))?;
        if fields.len() != values.len() {
            return Err(span.error("enum variant argument count mismatch"));
        }
        let mut bindings = BTreeMap::new();
        if !args.is_empty() {
            if args.len() != d.parameters.len() {
                return Err(span.error("enum type argument count mismatch"));
            }
            bindings.extend(
                d.parameters
                    .iter()
                    .zip(args)
                    .map(|(p, a)| (p.text.clone(), a.into())),
            );
        }
        if let Some(e) = expected {
            let pattern = if d.parameters.is_empty() {
                base.to_string()
            } else {
                format!(
                    "{base}<{}>",
                    d.parameters
                        .iter()
                        .map(|p| p.text.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            if !generics::unify(&pattern, e, &d.parameters, &mut bindings) {
                return Err(span.error("enum constructor type context mismatch"));
            }
        }
        for (field, value) in fields.iter().zip(values) {
            if !generics::unify(&field.text, &value.ty, &d.parameters, &mut bindings) {
                return Err(span.error("enum variant argument type mismatch"));
            }
        }
        let arguments = d
            .parameters
            .iter()
            .map(|p| {
                bindings.get(&p.text).cloned().ok_or_else(|| {
                    span.error(format!(
                        "cannot infer enum type parameter '{}'; add type arguments or annotation",
                        p.text
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ty = if arguments.is_empty() {
            base.into()
        } else {
            format!("{base}<{}>", arguments.join(","))
        };
        self.check_type(&Name {
            text: ty.clone(),
            span: span.clone(),
        })?;
        Ok(Some(self.emit(
            &ty,
            Rvalue::Variant {
                index,
                fields: values.to_vec(),
            },
        )))
    }
    pub(super) fn option_element(&self, ty: &str) -> Option<String> {
        self.enums
            .get(ty)
            .filter(|e| e.definition == "Option")
            .map(|e| e.arguments[0].clone())
    }
    pub(super) fn option_operation(
        &mut self,
        name: &str,
        args: &[Value],
        span: &Span,
    ) -> Result<Option<usize>, String> {
        let Some(first) = args.first() else {
            return Ok(None);
        };
        let Some(element) = self.option_element(&first.ty) else {
            return Ok(None);
        };
        let result = match name {
            "is_some" | "is_none" | "__bool__" | "__not__" => "Bool".to_string(),
            "value" => element,
            _ => return Ok(None),
        };
        if args.len() != 1 {
            return Err(span.error("Option operation takes one receiver"));
        }
        let native = self.types[&first.ty].clone();
        let result_native = self.types[&result].clone();
        let body = if name == "value" {
            "if(x->value.index()!=0)fail(loc,\"absent Option value\");return std::get<0>(x->value).field_0;".into()
        } else {
            format!(
                "return make_obj<bool>(x->value.index(){}0);",
                if matches!(name, "is_none" | "__not__") {
                    "!="
                } else {
                    "=="
                }
            )
        };
        self.cached_native(
            name,
            vec![first.ty.clone()],
            &result,
            |cpp| {
                format!(
                    "Obj<{result_native}> {cpp}(Obj<{native}> x,[[maybe_unused]] Loc loc){{{body}}}"
                )
            },
            span,
        )
        .map(Some)
    }
    pub(super) fn prepare_enum_copies(&mut self) -> Result<(), String> {
        let todo = self
            .enums
            .iter()
            .filter(|(ty, _)| !self.enum_copies.contains_key(*ty))
            .map(|(ty, e)| (ty.clone(), e.clone()))
            .collect::<Vec<_>>();
        for (ty, e) in todo {
            let mut variants = vec![];
            for (_, fields) in e.variants {
                let mut copies = vec![];
                for field in fields {
                    let index = self
                        .resolve(
                            "copy",
                            &[Value {
                                ty: field.text.clone(),
                                id: ValueId::Probe,
                            }],
                            None,
                            &field.span,
                        )?
                        .ok_or_else(|| field.span.error("enum payload requires copy"))?;
                    self.same(&field.text, &self.functions[index].result, &field.span)?;
                    self.used.insert(index);
                    copies.push(index);
                }
                variants.push(copies);
            }
            self.enum_copies.insert(ty, variants);
        }
        Ok(())
    }
    fn check_enum_cycle(
        &self,
        name: &Name,
        path: &mut Vec<String>,
        checked: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if path.len() > 64 || name.text.len() > 16_384 || path.contains(&name.text) {
            return Err(name
                .span
                .error("recursive enum type or type expansion limit exceeded"));
        }
        if checked.contains(&name.text) {
            return Ok(());
        }
        let (base, args) = type_parts(&name.text).unwrap_or((&name.text, vec![]));
        path.push(name.text.clone());
        let mut fields = vec![];
        if let Some(d) = self.enum_definitions.get(base) {
            let bindings = d
                .parameters
                .iter()
                .zip(&args)
                .map(|(p, a)| (p.text.clone(), (*a).to_string()))
                .collect();
            fields.extend(d.variants.iter().flat_map(|(_, fs)| fs).map(|f| Name {
                text: generics::substitute(&f.text, &bindings),
                span: f.span.clone(),
            }));
        } else if let Some(r) = self.records.get(&name.text) {
            fields.extend(r.fields.iter().map(|(_, t)| t.clone()));
        } else if let Some(r) = self.generic_records.get(base) {
            let bindings = r
                .parameters
                .iter()
                .zip(&args)
                .map(|(p, a)| (p.text.clone(), (*a).to_string()))
                .collect();
            fields.extend(r.fields.iter().map(|(_, f)| Name {
                text: generics::substitute(&f.text, &bindings),
                span: f.span.clone(),
            }));
        }
        // Traverse type arguments as well, including containers and phantom arguments.
        fields.extend(args.iter().map(|a| Name {
            text: (*a).into(),
            span: name.span.clone(),
        }));
        for field in fields {
            self.check_enum_cycle(&field, path, checked)?;
        }
        path.pop();
        checked.insert(name.text.clone());
        Ok(())
    }
    pub(super) fn enum_forwards(&self) -> String {
        self.enums
            .keys()
            .map(|ty| format!("struct {};\n", self.types[ty]))
            .collect()
    }
    pub(super) fn enum_cpp(&self) -> String {
        let mut code = String::new();
        for (ty, e) in &self.enums {
            let native = &self.types[ty];
            code += &format!("struct {native}{{\n");
            for (index, (_, fields)) in e.variants.iter().enumerate() {
                code += &format!("struct Variant{index}{{");
                for (i, f) in fields.iter().enumerate() {
                    code += &format!("Obj<{}> field_{i};", self.types[&f.text]);
                }
                code += "};\n";
            }
            code += &format!(
                "std::variant<{}> value;\n}};\n",
                (0..e.variants.len())
                    .map(|i| format!("Variant{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            code += &format!(
                "Obj<{native}> deep_copy(const Obj<{native}>& value,Loc loc=Loc{{\"<copy>\",0,0}});\n"
            );
        }
        code
    }
    pub(super) fn enum_copy_cpp(&self) -> String {
        let mut code = String::new();
        for (ty, variants) in &self.enum_copies {
            let native = &self.types[ty];
            code += &format!(
                "Obj<{native}> deep_copy(const Obj<{native}>& value,[[maybe_unused]] Loc loc){{auto snapshot=value->value;switch(snapshot.index()){{"
            );
            for (i, fields) in variants.iter().enumerate() {
                code += &format!("case {i}:{{");
                for (j, f) in fields.iter().enumerate() {
                    code += &format!(
                        "auto field_{j}={}(std::get<{i}>(snapshot).field_{j},loc);",
                        self.functions[*f].cpp
                    );
                }
                let args = (0..fields.len())
                    .map(|j| format!("std::move(field_{j})"))
                    .collect::<Vec<_>>()
                    .join(",");
                code += &format!("return rusk_enum_make<{native},{i}>({args});}}");
            }
            code += "default:fail(loc,\"invalid enum tag\");}}\n";
        }
        code
    }
}

impl Compiler {
    pub(super) fn match_expression(
        &mut self,
        source: &Expr,
        arms: &[crate::ast::MatchArm],
        expected: Option<&str>,
        span: &Span,
    ) -> Result<Value, String> {
        let value = self.expr(source, None)?;
        if self.diverged {
            return Ok(Value::never());
        }
        let e = self
            .enums
            .get(&value.ty)
            .cloned()
            .ok_or_else(|| span.error("match requires an enum value"))?;
        let mut covered = BTreeSet::new();
        let mut indices = vec![];
        let mut wildcard = false;
        for arm in arms {
            if wildcard {
                return Err(arm.name.span.error("unreachable match arm"));
            }
            if arm.name.text == "_" {
                if !arm.bindings.is_empty() {
                    return Err(arm.name.span.error("wildcard has no payload"));
                }
                if covered.len() == e.variants.len() {
                    return Err(arm.name.span.error("unreachable match arm"));
                }
                wildcard = true;
                indices.push(None);
                continue;
            }
            let variant = if let Some((owner, v)) = arm.name.text.rsplit_once("::") {
                if owner != value.ty && owner != e.definition {
                    return Err(arm.name.span.error("pattern belongs to another enum type"));
                }
                v
            } else {
                &arm.name.text
            };
            let index = e
                .variants
                .iter()
                .position(|(n, _)| n.text == variant)
                .ok_or_else(|| arm.name.span.error("unknown enum variant in pattern"))?;
            if !covered.insert(index) {
                return Err(arm.name.span.error("unreachable duplicate variant pattern"));
            }
            if arm.bindings.len() != e.variants[index].1.len() {
                return Err(arm.name.span.error("enum pattern payload count mismatch"));
            }
            indices.push(Some(index));
        }
        if !wildcard && covered.len() != e.variants.len() {
            let missing = e
                .variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered.contains(i))
                .map(|(_, (n, _))| n.text.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(span.error(format!("non-exhaustive match; missing: {missing}")));
        }
        self.match_arms(&value, arms, &indices, expected, span)
    }
    fn match_arms(
        &mut self,
        value: &Value,
        arms: &[crate::ast::MatchArm],
        indices: &[Option<usize>],
        expected: Option<&str>,
        span: &Span,
    ) -> Result<Value, String> {
        if arms.len() == 1 {
            return self.match_arm(value, &arms[0], indices[0], expected);
        }
        let condition = if let Some(index) = indices[0] {
            self.emit(
                "Bool",
                Rvalue::VariantTest {
                    owner: value.clone(),
                    index,
                },
            )
        } else {
            self.literal("Bool", "true", span)?
        };
        self.conditional(
            condition,
            expected,
            |c, h| c.match_arm(value, &arms[0], indices[0], h),
            |c, h| c.match_arms(value, &arms[1..], &indices[1..], h, span),
            span,
        )
    }
    fn match_arm(
        &mut self,
        value: &Value,
        arm: &crate::ast::MatchArm,
        index: Option<usize>,
        expected: Option<&str>,
    ) -> Result<Value, String> {
        // A block expression keeps bindings local and exports only the shared result.
        // Bind payloads before the body so replacing the original enum is safe.
        self.scopes.push(BTreeMap::new());
        let outer = std::mem::take(&mut self.pending_temps);
        let mut result = Value::never();
        let slot = self.temp();
        let body = self.capture(|c| {
            if let Some(index) = index {
                let fields = c.enums[&value.ty].variants[index].1.clone();
                for (field_index, (field, binding)) in fields.iter().zip(&arm.bindings).enumerate()
                {
                    let item = c.emit(
                        &field.text,
                        Rvalue::VariantField {
                            owner: value.clone(),
                            variant: index,
                            index: field_index,
                        },
                    );
                    c.declare_binding(binding, item)?;
                }
            }
            result = c.expr(&arm.body, expected)?;
            if !c.diverged {
                if let Some(expected) = expected {
                    c.same(expected, &result.ty, &arm.name.span)?;
                }
                c.body.push(Instruction::Rebind {
                    target: Place::Local(slot.clone()),
                    value: result.clone(),
                });
                c.release_temps();
            }
            Ok(())
        })?;
        self.scopes.pop();
        self.pending_temps = outer;
        if self.diverged {
            self.body.push(Instruction::Scope(body));
            return Ok(Value::never());
        }
        result.id = slot;
        self.body.push(Instruction::Declare {
            result: result.clone(),
            initial: None,
        });
        self.body.push(Instruction::Scope(body));
        self.pending_temps.push(result.id.clone());
        Ok(result)
    }
}
