use std::collections::HashSet;

use crate::model::{CodeModel, Function};

use super::{DiagramEmitter, MermaidTheme};

pub struct ZenumlEmitter;

impl ZenumlEmitter {
    fn format_call(call: &crate::model::CallExpr) -> String {
        let args_str = call
            .arguments
            .iter()
            .filter(|a| *a != "...")
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        match &call.receiver {
            Some(receiver) => format!("{}.{}({})", receiver, call.method, args_str),
            None => format!("{}({})", call.method, args_str),
        }
    }

    fn emit_calls(
        output: &mut String,
        func: &Function,
        all_functions: &[Function],
        visited: &mut HashSet<String>,
        indent: usize,
    ) {
        let pad = "    ".repeat(indent);
        for call in &func.calls {
            let call_str = Self::format_call(call);

            let nested_func = if !visited.contains(&call.method) {
                all_functions
                    .iter()
                    .find(|f| f.name == call.method && !f.calls.is_empty())
            } else {
                None
            };

            if let Some(nested) = nested_func {
                visited.insert(call.method.clone());
                output.push_str(&format!("{}{} {{\n", pad, call_str));
                Self::emit_calls(output, nested, all_functions, visited, indent + 1);
                output.push_str(&format!("{}}}\n", pad));
            } else {
                output.push_str(&format!("{}{}\n", pad, call_str));
            }
        }
    }
}

impl DiagramEmitter for ZenumlEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String {
        let mut output = theme.directive();
        output.push_str("zenuml\n");

        let interesting: Vec<&Function> = model
            .functions
            .iter()
            .filter(|f| {
                !f.calls.is_empty()
                    && matches!(
                        f.visibility,
                        crate::model::Visibility::Public | crate::model::Visibility::Internal
                    )
            })
            .collect();

        if interesting.is_empty() {
            return output;
        }

        for func in &interesting {
            let mut visited = HashSet::new();
            visited.insert(func.name.clone());

            output.push_str(&format!("    // {}\n", func.name));
            Self::emit_calls(&mut output, func, &model.functions, &mut visited, 1);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallExpr, Function, Visibility};

    #[test]
    fn test_emit_empty_model() {
        let emitter = ZenumlEmitter;
        let model = CodeModel::new();
        assert_eq!(
            emitter.emit(&model, &MermaidTheme::Default),
            "zenuml\n"
        );
    }

    #[test]
    fn test_emit_function_with_receiver_calls() {
        let emitter = ZenumlEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "process".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![
                    CallExpr {
                        receiver: Some("db".to_string()),
                        method: "connect".to_string(),
                        arguments: vec![],
                    },
                    CallExpr {
                        receiver: Some("db".to_string()),
                        method: "query".to_string(),
                        arguments: vec!["sql".to_string()],
                    },
                ],
                source_file: "process.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.starts_with("zenuml\n"));
        assert!(result.contains("db.connect()"));
        assert!(result.contains("db.query(sql)"));
    }
}
