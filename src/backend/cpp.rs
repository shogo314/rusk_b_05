use super::{
    intrinsics,
    ir::{Body, Instruction},
};

impl Body {
    pub fn cpp(&self, optimize: bool) -> String {
        let mut output = String::new();
        for instruction in &self.instructions {
            match instruction {
                Instruction::Code(code) => output.push_str(code),
                Instruction::Object {
                    result,
                    native,
                    expression,
                } => {
                    output.push_str(&format!("Obj<{native}> {} = {expression};\n", result.cpp));
                }
                Instruction::Literal {
                    result,
                    native,
                    expression,
                } => {
                    let constructor = if optimize
                        && matches!(result.ty.as_str(), "Int" | "Bool" | "Char" | "Unit")
                    {
                        format!("RuskTemporary<{native}>")
                    } else {
                        format!("make_obj<{native}>")
                    };
                    output.push_str(&format!(
                        "auto {} = {constructor}({expression});\n",
                        result.cpp
                    ));
                }
                Instruction::Call(call) => {
                    if optimize && let Some(code) = intrinsics::lower(call) {
                        if call.result.ty == "Unit" {
                            output.push_str(&format!("{code};\n"));
                            output.push_str(&format!(
                                "auto {} = RuskTemporary<Unit>(Unit{{}});\n",
                                call.result.cpp
                            ));
                        } else {
                            output.push_str(&format!(
                                "auto {} = RuskTemporary<{}>({code});\n",
                                call.result.cpp, call.native
                            ));
                        }
                    } else {
                        let mut args = call.args.iter().map(|v| v.cpp.clone()).collect::<Vec<_>>();
                        args.push(crate::compiler::location(&call.span));
                        let expression = format!("{}({})", call.cpp, args.join(", "));
                        if call.result.ty == "Unit" {
                            output.push_str(&format!("{expression};\n"));
                            let constructor = if optimize {
                                "RuskTemporary<Unit>"
                            } else {
                                "make_obj<Unit>"
                            };
                            output.push_str(&format!(
                                "auto {} = {constructor}(Unit{{}});\n",
                                call.result.cpp
                            ));
                        } else {
                            output.push_str(&format!("auto {} = {expression};\n", call.result.cpp));
                        }
                    }
                }
            }
        }
        output
    }
}
