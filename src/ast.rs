use crate::rusk_grammar_trait as g;
use parol_runtime::Token;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Span {
    pub file: String,
    pub line: u32,
    pub column: u32,
}
impl Span {
    pub fn token(t: &Token<'_>) -> Self {
        Self {
            file: t.location.file_name.display().to_string(),
            line: t.location.start_line,
            column: t.location.start_column,
        }
    }
    pub fn error(&self, message: impl std::fmt::Display) -> String {
        format!("{}:{}:{}: {}", self.file, self.line, self.column, message)
    }
}
#[derive(Clone, Debug)]
pub struct Name {
    pub text: String,
    pub span: Span,
}
fn name(n: &g::Identifier<'_>) -> Name {
    Name {
        text: n.identifier.text().into(),
        span: Span::token(&n.identifier),
    }
}
fn type_parameters(t: &g::TypeParameters<'_>) -> Vec<Name> {
    std::iter::once(t.identifier.as_ref())
        .chain(t.type_parameters_list.iter().map(|t| t.identifier.as_ref()))
        .map(name)
        .collect()
}
fn reference(t: &g::Reference<'_>) -> Name {
    let mut n = name(&t.identifier);
    if let Some(arguments) = &t.reference_opt {
        n.text = format!(
            "{}<{}>",
            n.text,
            type_arguments(&arguments.type_arguments)
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    n
}
fn type_arguments(t: &g::TypeArguments<'_>) -> Vec<Name> {
    std::iter::once(t.type_name.as_ref())
        .chain(t.type_arguments_list.iter().map(|t| t.type_name.as_ref()))
        .map(type_name)
        .collect()
}
fn type_name(t: &g::TypeName<'_>) -> Name {
    match t {
        g::TypeName::NamedType(t) => {
            let mut n = name(&t.named_type.identifier);
            if let Some(args) = &t.named_type.named_type_opt {
                n.text = format!(
                    "{}<{}>",
                    n.text,
                    type_arguments(&args.type_arguments)
                        .iter()
                        .map(|t| t.text.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            n
        }
        g::TypeName::TupleType(t) => {
            let t = &t.tuple_type;
            let types = std::iter::once(t.type_name.as_ref())
                .chain(std::iter::once(t.type_name0.as_ref()))
                .chain(t.tuple_type_list.iter().map(|t| t.type_name.as_ref()))
                .map(type_name)
                .collect::<Vec<_>>();
            Name {
                text: format!(
                    "Tuple<{}>",
                    types
                        .iter()
                        .map(|t| t.text.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                span: types[0].span.clone(),
            }
        }
    }
}
#[derive(Clone, Debug)]
pub enum Binding {
    Name(Name),
    Tuple(Vec<Binding>, Span),
}
impl Binding {
    pub fn span(&self) -> &Span {
        match self {
            Self::Name(n) => &n.span,
            Self::Tuple(_, span) => span,
        }
    }
}
fn binding(b: &g::Binding<'_>) -> Binding {
    match b {
        g::Binding::Identifier(n) => Binding::Name(name(&n.identifier)),
        g::Binding::LParenBindingCommaBindingBindingListRParen(t) => Binding::Tuple(
            std::iter::once(t.binding.as_ref())
                .chain(std::iter::once(t.binding0.as_ref()))
                .chain(t.binding_list.iter().map(|b| b.binding.as_ref()))
                .map(binding)
                .collect(),
            Span::token(&t.l_paren),
        ),
    }
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Literal(String, String),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Repeat(Box<Expr>, Box<Expr>),
    Range(Box<Expr>, Box<Expr>),
    Logical(String, Box<Expr>, Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, Name),
    Record(Name, Vec<(Name, Expr)>),
    Var(Name),
    Call(Name, Vec<Expr>),
    Invoke(Box<Expr>, Vec<Expr>),
    Lambda(Vec<(Name, Name)>, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Unary(String, Box<Expr>),
    Assign(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug)]
pub enum Initializer {
    Shared(Expr),
    Copied(Expr),
}
#[derive(Clone, Debug)]
pub enum Stmt {
    Include(String, Span),
    Struct(Name, Vec<Name>, Vec<(Name, Name)>),
    Type(Name, BTreeMap<String, String>),
    Function(
        Name,
        Vec<(Name, Name)>,
        Option<Name>,
        BTreeMap<String, String>,
    ),
    UserFunction(Name, Vec<Name>, Vec<(Name, Name)>, Option<Name>, Box<Stmt>),
    Return(Span, Option<Expr>),
    Symbol(Name, BTreeMap<String, String>),
    Let(Binding, Option<Name>, Option<Initializer>),
    Input(Vec<(Name, Name, Vec<Expr>)>),
    Output(Vec<Expr>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(Binding, Expr, Box<Stmt>, Option<Box<Stmt>>),
    Loop(Box<Stmt>),
    Break(Span),
    Continue(Span),
    Expr(Expr),
}
fn props(p: &g::Properties<'_>) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for entry in &p.properties_list {
        let p = &entry.property;
        let n = name(&p.identifier);
        let value = serde_json::from_str(p.string_literal.string_literal.text())
            .map_err(|e| n.span.error(e))?;
        if result.insert(n.text.clone(), value).is_some() {
            return Err(n.span.error(format!("duplicate property '{}'", n.text)));
        }
    }
    Ok(result)
}
pub fn lower(root: &g::Rusk<'_>) -> Result<Vec<Stmt>, String> {
    root.rusk_list.iter().map(|s| stmt(&s.statement)).collect()
}
fn stmt(s: &g::Statement<'_>) -> Result<Stmt, String> {
    Ok(match s {
        g::Statement::Include(x) => Stmt::Include(
            serde_json::from_str(x.include.string_literal.string_literal.text())
                .map_err(|e| Span::token(&x.include.string_literal.string_literal).error(e))?,
            Span::token(&x.include.string_literal.string_literal),
        ),
        g::Statement::StructDef(x) => Stmt::Struct(
            name(&x.struct_def.identifier),
            x.struct_def
                .struct_def_opt
                .as_ref()
                .map(|p| type_parameters(&p.type_parameters))
                .unwrap_or_default(),
            x.struct_def
                .struct_def_list
                .iter()
                .map(|field| {
                    (
                        name(&field.struct_field.identifier),
                        type_name(&field.struct_field.type_name),
                    )
                })
                .collect(),
        ),
        g::Statement::ExternType(x) => Stmt::Type(
            type_name(&x.extern_type.type_name),
            props(&x.extern_type.properties)?,
        ),
        g::Statement::ExternSymbol(x) => Stmt::Symbol(
            name(&x.extern_symbol.identifier),
            props(&x.extern_symbol.properties)?,
        ),
        g::Statement::ExternFn(x) => {
            let x = &x.extern_fn;
            let mut params = vec![];
            if let Some(p) = &x.parameters.parameters_opt {
                params.push((
                    name(&p.parameter.identifier),
                    type_name(&p.parameter.type_name),
                ));
                params.extend(p.parameters_opt_list.iter().map(|p| {
                    (
                        name(&p.parameter.identifier),
                        type_name(&p.parameter.type_name),
                    )
                }));
            }
            Stmt::Function(
                name(&x.identifier),
                params,
                x.extern_fn_opt.as_ref().map(|r| type_name(&r.type_name)),
                props(&x.properties)?,
            )
        }
        g::Statement::Function(x) => {
            let x = &x.function;
            let mut params = vec![];
            if let Some(p) = &x.parameters.parameters_opt {
                params.push((
                    name(&p.parameter.identifier),
                    type_name(&p.parameter.type_name),
                ));
                params.extend(p.parameters_opt_list.iter().map(|p| {
                    (
                        name(&p.parameter.identifier),
                        type_name(&p.parameter.type_name),
                    )
                }));
            }
            Stmt::UserFunction(
                name(&x.identifier),
                x.function_opt
                    .as_ref()
                    .map(|p| type_parameters(&p.type_parameters))
                    .unwrap_or_default(),
                params,
                x.function_opt0.as_ref().map(|r| type_name(&r.type_name)),
                Box::new(block(&x.block)?),
            )
        }
        g::Statement::Return(x) => Stmt::Return(
            Span::token(&x.r#return.r#return),
            x.r#return
                .return_opt
                .as_ref()
                .map(|x| expr(&x.expr))
                .transpose()?,
        ),
        g::Statement::Let(x) => {
            let x = &x.r#let;
            Stmt::Let(
                binding(&x.binding),
                x.let_opt.as_ref().map(|t| type_name(&t.type_name)),
                x.let_opt0
                    .as_ref()
                    .map(|e| -> Result<Initializer, String> {
                        let value = expr(&e.expr)?;
                        Ok(match e.let_init_op.as_ref() {
                            g::LetInitOp::ColonEqu(_) => Initializer::Shared(value),
                            g::LetInitOp::Equ(_) => Initializer::Copied(value),
                        })
                    })
                    .transpose()?,
            )
        }
        g::Statement::Input(x) => {
            let mut items = vec![];
            for item in std::iter::once(x.input.input_item.as_ref())
                .chain(x.input.input_list.iter().map(|i| i.input_item.as_ref()))
            {
                let ty = type_name(&item.type_name);
                for item in std::iter::once(item.input_name.as_ref())
                    .chain(item.input_item_list.iter().map(|i| i.input_name.as_ref()))
                {
                    let args = item
                        .input_name_opt
                        .as_ref()
                        .map(|x| list(&x.expr_list))
                        .transpose()?
                        .unwrap_or_default();
                    items.push((name(&item.identifier), ty.clone(), args));
                }
            }
            Stmt::Input(items)
        }
        g::Statement::Output(x) => Stmt::Output(list(&x.output.expr_list)?),
        g::Statement::Block(x) => block(&x.block)?,
        g::Statement::If(x) => Stmt::If(
            expr(&x.r#if.expr)?,
            Box::new(block(&x.r#if.block)?),
            x.r#if
                .if_opt
                .as_ref()
                .map(|x| block(&x.block).map(Box::new))
                .transpose()?,
        ),
        g::Statement::For(x) => Stmt::For(
            binding(&x.r#for.binding),
            expr(&x.r#for.expr)?,
            Box::new(block(&x.r#for.block)?),
            x.r#for
                .for_opt
                .as_ref()
                .map(|x| block(&x.block).map(Box::new))
                .transpose()?,
        ),
        g::Statement::While(x) => Stmt::While(
            expr(&x.r#while.expr)?,
            Box::new(block(&x.r#while.block)?),
            x.r#while
                .while_opt
                .as_ref()
                .map(|x| block(&x.block).map(Box::new))
                .transpose()?,
        ),
        g::Statement::Loop(x) => Stmt::Loop(Box::new(block(&x.r#loop.block)?)),
        g::Statement::Break(x) => Stmt::Break(Span::token(&x.r#break.r#break)),
        g::Statement::Continue(x) => Stmt::Continue(Span::token(&x.r#continue.r#continue)),
        g::Statement::ExprSemicolon(x) => Stmt::Expr(expr(&x.expr)?),
    })
}
fn block(b: &g::Block<'_>) -> Result<Stmt, String> {
    Ok(Stmt::Block(
        b.block_list
            .iter()
            .map(|s| stmt(&s.statement))
            .collect::<Result<_, _>>()?,
    ))
}
fn list(l: &g::ExprList<'_>) -> Result<Vec<Expr>, String> {
    let Some(x) = &l.expr_list_opt else {
        return Ok(vec![]);
    };
    std::iter::once(x.expr.as_ref())
        .chain(x.expr_list_opt_list.iter().map(|i| i.expr.as_ref()))
        .map(expr)
        .collect()
}
fn expr(e: &g::Expr<'_>) -> Result<Expr, String> {
    let e = match e {
        g::Expr::Lambda(x) => return lambda(&x.lambda),
        g::Expr::RangeExprOpt(x) => x,
    };
    let left = range(&e.range)?;
    if let Some(x) = &e.expr_opt {
        let (op, t) = match x.assign_op.as_ref() {
            g::AssignOp::ColonEqu(x) => (":=", &x.colon_equ),
            g::AssignOp::Equ(x) => ("=", &x.equ),
            g::AssignOp::PlusEqu(x) => ("+=", &x.plus_equ),
            g::AssignOp::MinusEqu(x) => ("-=", &x.minus_equ),
            g::AssignOp::StarEqu(x) => ("*=", &x.star_equ),
            g::AssignOp::SlashEqu(x) => ("/=", &x.slash_equ),
            g::AssignOp::PercentEqu(x) => ("%=", &x.percent_equ),
        };
        return Ok(Expr {
            kind: ExprKind::Assign(op.into(), Box::new(left), Box::new(expr(&x.expr)?)),
            span: Span::token(t),
        });
    }
    Ok(left)
}
fn range(e: &g::Range<'_>) -> Result<Expr, String> {
    let left = logical_or(&e.logical_or)?;
    if let Some(right) = &e.range_opt {
        let span = left.span.clone();
        Ok(Expr {
            kind: ExprKind::Range(Box::new(left), Box::new(logical_or(&right.logical_or)?)),
            span,
        })
    } else {
        Ok(left)
    }
}
fn logical_or(e: &g::LogicalOr<'_>) -> Result<Expr, String> {
    let mut left = logical_and(&e.logical_and)?;
    for right in &e.logical_or_list {
        let span = left.span.clone();
        left = Expr {
            kind: ExprKind::Logical(
                "||".into(),
                Box::new(left),
                Box::new(logical_and(&right.logical_and)?),
            ),
            span,
        };
    }
    Ok(left)
}
fn logical_and(e: &g::LogicalAnd<'_>) -> Result<Expr, String> {
    let mut left = bit_or(&e.bit_or)?;
    for right in &e.logical_and_list {
        let span = left.span.clone();
        left = Expr {
            kind: ExprKind::Logical(
                "&&".into(),
                Box::new(left),
                Box::new(bit_or(&right.bit_or)?),
            ),
            span,
        };
    }
    Ok(left)
}
fn bit_or(e: &g::BitOr<'_>) -> Result<Expr, String> {
    let mut left = bit_xor(&e.bit_xor)?;
    for right in &e.bit_or_list {
        let span = left.span.clone();
        left = Expr {
            kind: ExprKind::Binary(
                "|".into(),
                Box::new(left),
                Box::new(bit_xor(&right.bit_xor)?),
            ),
            span,
        };
    }
    Ok(left)
}
fn bit_xor(e: &g::BitXor<'_>) -> Result<Expr, String> {
    let mut left = bit_and(&e.bit_and)?;
    for right in &e.bit_xor_list {
        let span = left.span.clone();
        left = Expr {
            kind: ExprKind::Binary(
                "^".into(),
                Box::new(left),
                Box::new(bit_and(&right.bit_and)?),
            ),
            span,
        };
    }
    Ok(left)
}
fn bit_and(e: &g::BitAnd<'_>) -> Result<Expr, String> {
    let mut left = compare(&e.compare)?;
    for right in &e.bit_and_list {
        let span = left.span.clone();
        left = Expr {
            kind: ExprKind::Binary(
                "&".into(),
                Box::new(left),
                Box::new(compare(&right.compare)?),
            ),
            span,
        };
    }
    Ok(left)
}
fn compare(e: &g::Compare<'_>) -> Result<Expr, String> {
    let left = add(&e.add)?;
    let Some(x) = &e.compare_opt else {
        return Ok(left);
    };
    let (op, t) = match x.compare_op.as_ref() {
        g::CompareOp::EquEqu(x) => ("==", &x.equ_equ),
        g::CompareOp::BangEqu(x) => ("!=", &x.bang_equ),
        g::CompareOp::LT(x) => ("<", &x.l_t),
        g::CompareOp::LTEqu(x) => ("<=", &x.l_t_equ),
        g::CompareOp::GT(x) => (">", &x.g_t),
        g::CompareOp::GTEqu(x) => (">=", &x.g_t_equ),
    };
    Ok(Expr {
        kind: ExprKind::Binary(op.into(), Box::new(left), Box::new(add(&x.add)?)),
        span: Span::token(t),
    })
}
fn add(e: &g::Add<'_>) -> Result<Expr, String> {
    let mut left = mul(&e.mul)?;
    for x in &e.add_list {
        let (op, t) = match x.add_op.as_ref() {
            g::AddOp::Plus(x) => ("+", &x.plus),
            g::AddOp::Minus(x) => ("-", &x.minus),
        };
        left = Expr {
            kind: ExprKind::Binary(op.into(), Box::new(left), Box::new(mul(&x.mul)?)),
            span: Span::token(t),
        };
    }
    Ok(left)
}
fn mul(e: &g::Mul<'_>) -> Result<Expr, String> {
    let mut left = unary(&e.unary)?;
    for x in &e.mul_list {
        let (op, t) = match x.mul_op.as_ref() {
            g::MulOp::Star(x) => ("*", &x.star),
            g::MulOp::Slash(x) => ("/", &x.slash),
            g::MulOp::Percent(x) => ("%", &x.percent),
        };
        left = Expr {
            kind: ExprKind::Binary(op.into(), Box::new(left), Box::new(unary(&x.unary)?)),
            span: Span::token(t),
        };
    }
    Ok(left)
}
fn unary(e: &g::Unary<'_>) -> Result<Expr, String> {
    match e {
        g::Unary::UnaryOpUnary(x) => {
            let (op, t) = match x.unary_op.as_ref() {
                g::UnaryOp::Plus(x) => ("+", &x.plus),
                g::UnaryOp::Minus(x) => ("-", &x.minus),
                g::UnaryOp::Bang(x) => ("!", &x.bang),
            };
            Ok(Expr {
                kind: ExprKind::Unary(op.into(), Box::new(unary(&x.unary)?)),
                span: Span::token(t),
            })
        }
        g::Unary::Postfix(x) => postfix(&x.postfix),
    }
}
fn postfix(p: &g::Postfix<'_>) -> Result<Expr, String> {
    let mut value = primary(&p.primary)?;
    for suffix in &p.postfix_list {
        value = match suffix.suffix.as_ref() {
            g::Suffix::Index(x) => Expr {
                kind: ExprKind::Index(Box::new(value), Box::new(expr(&x.index.expr)?)),
                span: Span::token(&x.index.l_bracket),
            },
            g::Suffix::Member(x) => {
                let n = name(&x.member.identifier);
                Expr {
                    span: n.span.clone(),
                    kind: ExprKind::Field(Box::new(value), n),
                }
            }
            g::Suffix::Invoke(x) => {
                let span = value.span.clone();
                let args = list(&x.invoke.expr_list)?;
                let kind = match value.kind {
                    ExprKind::Var(n) => ExprKind::Call(n, args),
                    _ => ExprKind::Invoke(Box::new(value), args),
                };
                Expr { span, kind }
            }
        };
    }
    Ok(value)
}
fn primary(e: &g::Primary<'_>) -> Result<Expr, String> {
    Ok(match e {
        g::Primary::Array(x) => {
            let kind = if let Some(items) = &x.array.array_opt {
                let first = expr(&items.expr)?;
                match items.array_tail.as_ref() {
                    g::ArrayTail::SemicolonExpr(x) => {
                        ExprKind::Repeat(Box::new(first), Box::new(expr(&x.expr)?))
                    }
                    g::ArrayTail::ArrayTailList(x) => {
                        let mut values = vec![first];
                        values.extend(
                            x.array_tail_list
                                .iter()
                                .map(|x| expr(&x.expr))
                                .collect::<Result<Vec<_>, _>>()?,
                        );
                        ExprKind::Array(values)
                    }
                }
            } else {
                ExprKind::Array(vec![])
            };
            Expr {
                kind,
                span: Span::token(&x.array.l_bracket),
            }
        }
        g::Primary::LParenExprPrimaryOpt0RParen(x) => {
            let first = expr(&x.expr)?;
            if let Some(tail) = &x.primary_opt0 {
                let mut values = vec![first];
                values.push(expr(&tail.tuple_tail.expr)?);
                for item in &tail.tuple_tail.tuple_tail_list {
                    values.push(expr(&item.expr)?);
                }
                Expr {
                    span: Span::token(&x.l_paren),
                    kind: ExprKind::Tuple(values),
                }
            } else {
                return Ok(first);
            }
        }
        g::Primary::ReferencePrimaryOpt(x) => {
            let n = reference(&x.reference);
            Expr {
                span: n.span.clone(),
                kind: if let Some(record) = &x.primary_opt {
                    let fields = &record.record_fields;
                    let values = std::iter::once(fields.field_value.as_ref())
                        .chain(
                            fields
                                .record_fields_list
                                .iter()
                                .map(|f| f.field_value.as_ref()),
                        )
                        .map(|f| Ok((name(&f.identifier), expr(&f.expr)?)))
                        .collect::<Result<_, String>>()?;
                    ExprKind::Record(n, values)
                } else {
                    ExprKind::Var(n)
                },
            }
        }
        g::Primary::Literal(x) => {
            let (ty, t) = match x.literal.as_ref() {
                g::Literal::IntLiteral(x) => ("Int", &x.int_literal.int_literal),
                g::Literal::StringLiteral(x) => ("Str", &x.string_literal.string_literal),
                g::Literal::CharLiteral(x) => ("Char", &x.char_literal.char_literal),
                g::Literal::BoolLiteral(x) => (
                    "Bool",
                    match x.bool_literal.as_ref() {
                        g::BoolLiteral::True(x) => &x.r#true,
                        g::BoolLiteral::False(x) => &x.r#false,
                    },
                ),
            };
            Expr {
                kind: ExprKind::Literal(ty.into(), t.text().into()),
                span: Span::token(t),
            }
        }
    })
}

fn lambda(e: &g::Lambda<'_>) -> Result<Expr, String> {
    let mut parameters = vec![];
    if let Some(params) = &e.parameters.parameters_opt {
        parameters.push((
            name(&params.parameter.identifier),
            type_name(&params.parameter.type_name),
        ));
        for p in &params.parameters_opt_list {
            parameters.push((
                name(&p.parameter.identifier),
                type_name(&p.parameter.type_name),
            ));
        }
    }
    Ok(Expr {
        kind: ExprKind::Lambda(parameters, Box::new(expr(&e.expr)?)),
        span: Span::token(&e.l_paren),
    })
}
