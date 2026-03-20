use std::collections::HashSet;

use crate::model::{CodeModel, Function};

use super::{DiagramEmitter, MermaidTheme};

pub struct SequenceEmitter;

impl SequenceEmitter {
    /// Format a call as a message label: `method(arg1, arg2)`
    fn format_message(call: &crate::model::CallExpr) -> String {
        let args_str = call
            .arguments
            .iter()
            .filter(|a| *a != "...")
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}({})", call.method, args_str)
    }

    /// Emit sequence messages for a function's calls.
    /// `caller` is the participant name of the calling context.
    fn emit_calls(
        output: &mut String,
        caller: &str,
        func: &Function,
        all_functions: &[Function],
        visited: &mut HashSet<String>,
    ) {
        for call in &func.calls {
            let target = call
                .receiver
                .as_deref()
                .unwrap_or(&call.method);
            let message = Self::format_message(call);

            // Emit the call arrow
            output.push_str(&format!("    {}->>{}:{}\n", caller, target, message));

            // If the called method resolves to a known function, recurse into it
            if !visited.contains(&call.method) {
                if let Some(nested) = all_functions
                    .iter()
                    .find(|f| f.name == call.method && !f.calls.is_empty())
                {
                    visited.insert(call.method.clone());
                    output.push_str(&format!("    activate {}\n", target));
                    Self::emit_calls(output, target, nested, all_functions, visited);
                    output.push_str(&format!("    deactivate {}\n", target));
                }
            }
        }
    }
}

impl DiagramEmitter for SequenceEmitter {
    fn emit(&self, model: &CodeModel, theme: &MermaidTheme) -> String {
        let mut output = theme.directive();
        output.push_str("sequenceDiagram\n");

        // Only emit public/internal functions that have calls
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

        // Collect all unique participants
        let mut participants = Vec::new();
        let mut seen = HashSet::new();
        for func in &interesting {
            if seen.insert(func.name.clone()) {
                participants.push(func.name.clone());
            }
            for call in &func.calls {
                let target = call.receiver.as_deref().unwrap_or(&call.method);
                if seen.insert(target.to_string()) {
                    participants.push(target.to_string());
                }
            }
        }
        for p in &participants {
            output.push_str(&format!("    participant {}\n", p));
        }

        // Emit each top-level function's call sequence
        for func in &interesting {
            let mut visited = HashSet::new();
            visited.insert(func.name.clone());

            output.push_str(&format!("    Note over {}: {}\n", func.name, func.name));
            Self::emit_calls(&mut output, &func.name, func, &model.functions, &mut visited);
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
        let emitter = SequenceEmitter;
        let model = CodeModel::new();
        assert_eq!(
            emitter.emit(&model, &MermaidTheme::Default),
            "sequenceDiagram\n"
        );
    }

    #[test]
    fn test_emit_function_with_receiver_calls() {
        let emitter = SequenceEmitter;
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
        assert!(result.starts_with("sequenceDiagram\n"));
        assert!(result.contains("participant process"));
        assert!(result.contains("participant db"));
        assert!(result.contains("process->>db:connect()"));
        assert!(result.contains("process->>db:query(sql)"));
    }

    #[test]
    fn test_emit_function_with_no_calls_produces_nothing() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "noop".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![],
                source_file: "noop.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert_eq!(result, "sequenceDiagram\n");
    }

    #[test]
    fn test_emit_nested_calls_with_activate() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![
                Function {
                    name: "main".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![CallExpr {
                        receiver: Some("App".to_string()),
                        method: "setup".to_string(),
                        arguments: vec![],
                    }],
                    source_file: "main.rs".to_string(),
                },
                Function {
                    name: "setup".to_string(),
                    parameters: vec![],
                    return_type: None,
                    visibility: Visibility::Public,
                    calls: vec![CallExpr {
                        receiver: Some("Config".to_string()),
                        method: "load".to_string(),
                        arguments: vec!["path".to_string()],
                    }],
                    source_file: "setup.rs".to_string(),
                },
            ],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("main->>App:setup()"));
        assert!(result.contains("activate App"));
        assert!(result.contains("App->>Config:load(path)"));
        assert!(result.contains("deactivate App"));
    }

    #[test]
    fn test_free_function_call_uses_method_as_target() {
        let emitter = SequenceEmitter;
        let model = CodeModel {
            entities: vec![],
            functions: vec![Function {
                name: "main".to_string(),
                parameters: vec![],
                return_type: None,
                visibility: Visibility::Public,
                calls: vec![CallExpr {
                    receiver: None,
                    method: "initialize".to_string(),
                    arguments: vec![],
                }],
                source_file: "main.rs".to_string(),
            }],
            relationships: vec![],
        };

        let result = emitter.emit(&model, &MermaidTheme::Default);
        assert!(result.contains("main->>initialize:initialize()"));
    }
}
