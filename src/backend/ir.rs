use crate::ast::Span;

#[derive(Clone)]
pub(crate) struct Value {
    pub cpp: String,
    pub ty: String,
}

pub(crate) struct Call {
    pub name: String,
    pub cpp: String,
    pub trusted: bool,
    pub args: Vec<Value>,
    pub span: Span,
    pub result: Value,
    pub native: String,
}

pub(crate) enum Instruction {
    /// Control flow, declarations, and aggregate/alias operations. These retain
    /// their checked source order; opaque C++ is never inspected by optimizers.
    Code(String),
    Object {
        result: Value,
        native: String,
        expression: String,
    },
    Literal {
        result: Value,
        native: String,
        expression: String,
    },
    Call(Call),
}

#[derive(Default)]
pub(crate) struct Body {
    pub instructions: Vec<Instruction>,
}

impl Body {
    pub fn push_str(&mut self, code: &str) {
        if let Some(Instruction::Code(previous)) = self.instructions.last_mut() {
            previous.push_str(code);
        } else {
            self.instructions.push(Instruction::Code(code.into()));
        }
    }

    pub fn clear(&mut self) {
        self.instructions.clear();
    }
}
